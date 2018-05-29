# -*- coding: utf-8 -*-
# frozen_string_literal: true
#
# Copyright (C) 2016 Mark Lee
#
# This program is free software: you can redistribute it and/or modify
# it under the terms of the GNU Affero General Public License as published by
# the Free Software Foundation, either version 3 of the License, or
# (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU Affero General Public License for more details.
#
# You should have received a copy of the GNU Affero General Public License
# along with this program.  If not, see <http://www.gnu.org/licenses/>.

require 'json'
require 'net/http'
require 'nokogiri'
require 'rack/request'
require 'rack/response'
require 'redis'
require 'semverse'

INDEX_HTML = File.join(File.dirname(File.dirname(__FILE__)), 'index.html').freeze
MIN_BUNDLER_VERSION = ENV.fetch('MIN_BUNDLER_VERSION', '1.16.0').freeze
REGEX = /BUNDLER_VERSION += "(.+?)"/
RUBY_LANGPACK_RELEASES_URL = 'https://github.com/heroku/heroku-buildpack-ruby/releases.atom'
RUBY_LANGPACK_URL = 'https://raw.githubusercontent.com/heroku/heroku-buildpack-ruby/%s/lib/' \
                    'language_pack/ruby.rb'

#
# Rack handler
#
class HasBuildpackBundlerUpgradedYet
  attr_reader :redis

  def initialize
    @redis = Redis.new(url: ENV['REDIS_URL']) if ENV.key?('REDIS_URL')
  end

  def http_get(uri)
    Net::HTTP.get(URI(uri))
  end

  def latest_buildpack_release
    releases = http_get(RUBY_LANGPACK_RELEASES_URL)
    xml = Nokogiri::XML(releases)
    xml.remove_namespaces!
    release = xml.xpath('/feed/entry/id/text()').first.to_s.split('/').last
    redis&.setex('latest_buildpack_release', 3600, release)

    release
  end

  def bundler_version_from_ruby_buildpack
    buildpack_release = redis&.get('latest_buildpack_release') || latest_buildpack_release
    ruby_langpack_url = RUBY_LANGPACK_URL % buildpack_release
    ruby_file = http_get(ruby_langpack_url)
    if (match = REGEX.match(ruby_file))
      bundler_version = match[1]
      redis&.hsetnx('bundler_version', buildpack_release, bundler_version)

      bundler_version
    end
  end

  def bundler_upgraded?
    if (version_from_buildpack = bundler_version_from_ruby_buildpack)
      constraint = Semverse::Constraint.new(">=#{MIN_BUNDLER_VERSION}")
      version = Semverse::Version.new(version_from_buildpack)
      constraint.satisfies?(version)
    else
      'WTF'
    end
  end

  def html_result(result)
    if result
      '<p class="yes"><i class="emoji"></i>Yes</p>'
    else
      '<p class="no"><i class="emoji"></i>No</p>'
    end
  end

  def to_html(result)
    File.open(INDEX_HTML) do |f|
      index = f.read
      index.gsub!('{{ is_bundler_upgraded }}', html_result(result))
      index.gsub!('{{ MIN_BUNDLER_VERSION }}', MIN_BUNDLER_VERSION)

      index
    end
  rescue => e
    "HTML NOT FOUND: #{e}"
  end

  def to_json(data)
    JSON.generate(result: data)
  end

  def accepts?(env, content_type)
    Rack::Utils.best_q_match(env['HTTP_ACCEPT'], [content_type])
  end

  def accepts_html?(env)
    accepts?(env, 'text/html')
  end

  def accepts_json?(env)
    accepts?(env, 'application/json')
  end

  def error(msg, status)
    Rack::Response.new(msg, status, 'Content-Type' => 'text/plain')
  end

  def call(env)
    request = Rack::Request.new(env)
    return error('GETs only', 405) unless request.get?
    return error('Root URL only', 404) unless request.path == '/'
    if accepts_json?(env)
      mime_type = 'application/json'
      serialize = :to_json
    elsif accepts_html?(env)
      mime_type = 'text/html'
      serialize = :to_html
    else
      return error('JSON or HTML output only', 406) # Not Acceptable
    end

    Rack::Response.new(send(serialize, bundler_upgraded?), 200, 'Content-Type' => mime_type)
  end
end
