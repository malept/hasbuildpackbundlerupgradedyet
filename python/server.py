#!/usr/bin/env python
# -*- coding: utf-8 -*-
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

import json
from lxml import etree
import os
import re
from redis import StrictRedis
import requests
import semver
from werkzeug.exceptions import MethodNotAllowed, NotAcceptable, NotFound
from werkzeug.wrappers import Request, Response

INDEX_HTML = os.path.abspath(os.path.join(os.path.dirname(__file__),
                                          '..',
                                          'index.html'))
MIN_BUNDLER_VERSION = os.environ.get('MIN_BUNDLER_VERSION', '1.12.0')
REGEX = re.compile(r'BUNDLER_VERSION += "(.+?)"')
RUBY_LANGPACK_RELEASES_URL = '''\
https://github.com/heroku/heroku-buildpack-ruby/releases.atom\
'''
RUBY_LANGPACK_URL = '''\
https://raw.githubusercontent.com/heroku/heroku-buildpack-ruby/{}/\
lib/language_pack/ruby.rb\
'''


def latest_buildpack_release(redis):
    releases = requests.get(RUBY_LANGPACK_RELEASES_URL).text
    xml = etree.fromstring(releases.encode('utf-8'))
    nsmap = {'atom': xml.nsmap[None]}
    release = xml.xpath('atom:entry/atom:id/text()', namespaces=nsmap)[0].rsplit('/', 1)[1]
    if redis:
        redis.setex('latest_buildpack_release', 3600, release)
    return release


def bundler_version_from_ruby_buildpack(redis):
    buildpack_release = None
    if redis:
        buildpack_release = redis.get('latest_buildpack_release')
    if not buildpack_release:
        buildpack_release = latest_buildpack_release(redis)

    ruby_langpack_url = RUBY_LANGPACK_URL.format(buildpack_release)
    ruby_file = requests.get(ruby_langpack_url).text
    match = REGEX.search(ruby_file)
    if match:
        bundler_version = match.group(1)
        if redis:
           redis.hsetnx('bundler_version', buildpack_release, bundler_version)
        return bundler_version
    else:
       return None


def is_bundler_upgraded(redis):
    version_from_buildpack = bundler_version_from_ruby_buildpack(redis)
    if version_from_buildpack is None:
        return False
    match = '>={}'.format(MIN_BUNDLER_VERSION)
    return semver.match(version_from_buildpack, match)


def html_result(result):
    if result:
        return '<p class="yes"><i class="emoji"></i>Yes</p>'
    else:
        return '<p class="no"><i class="emoji"></i>No</p>'


def to_html(result):
    try:
        with open(INDEX_HTML) as f:
            index = f.read()
        index = index.replace('{{ is_bundler_upgraded }}', html_result(result))
        return index.replace('{{ MIN_BUNDLER_VERSION }}', MIN_BUNDLER_VERSION)
    except IOError as e:
        return 'HTML NOT FOUND: {}'.format(e)


def to_json(data):
    return json.dumps({'result': data})


def application(environ, start_response):
    request = Request(environ)
    if request.method != 'GET':
        raise MethodNotAllowed('GETs only')
    if request.path != '/':
        raise NotFound('Root URL only')
    if request.accept_mimetypes.accept_json:
        mime_type = 'application/json'
        serialize = to_json
    elif request.accept_mimetypes.accept_html:
        mime_type = 'text/html'
        serialize = to_html
    else:
        raise NotAcceptable('JSON or HTML output only')
    redis_url = os.environ.get('REDIS_URL')
    if redis_url:
        redis = StrictRedis.from_url(redis_url)
    else:
        redis = None
    response = Response(serialize(is_bundler_upgraded(redis)), mimetype=mime_type)
    return response(environ, start_response)
