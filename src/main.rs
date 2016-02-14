// Copyright (C) 2016 Mark Lee

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.

// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <http://www.gnu.org/licenses/>.

extern crate env_logger;
extern crate hyper;
#[macro_use]
extern crate log;
extern crate redis;
extern crate regex;
extern crate rquery;
extern crate semver;
extern crate serde;
extern crate serde_json;

use hyper::{Client as HTTPClient, Get, Server};
use hyper::header::{Accept, ContentType};
use hyper::server::{Request, Response};
use hyper::uri::RequestUri::AbsolutePath;
use redis::Commands;
use regex::Regex;
use rquery::Document;
use semver::Version;
use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io;
use std::io::Read;

const DEFAULT_SERVER_PORT: u32 = 9000;
const MIN_BUNDLER_VERSION: &'static str = "1.11.0";
const RUBY_LANGPACK_RELEASES_URL: &'static str = "https://github.\
                                                  com/heroku/heroku-buildpack-ruby/releases.atom";

fn download_url(http: &HTTPClient, url: &str) -> io::Result<String> {
    let mut resp = http.get(url).send().expect("Could not send HTTP request");
    let mut body = String::new();
    match resp.read_to_string(&mut body) {
        Err(error) => Err(error),
        _ => Ok(body),
    }
}

fn download_latest_buildpack_release(http: &HTTPClient) -> String {
    let xml = download_url(&http, RUBY_LANGPACK_RELEASES_URL)
                  .expect("Could not download Atom releases!");
    let atom_doc = Document::new_from_xml_string(&xml[..]).expect("Could not parse Atom releases!");
    let latest_tag = atom_doc.select("entry title")
                             .expect("Could not find latest buildpack tag!");
    latest_tag.text().clone()
}

fn redis_cache_value(redis: &redis::Connection, key: &str, value: &str) -> String {
    let set_result: redis::RedisResult<()> = redis.set_ex(key, value, 3600);
    if let Err(_) = set_result {
        warn!("Cannot set {} in Redis, ignoring", key);
    }
    value.to_owned()
}

fn redis_cache_hash_value(redis: &redis::Connection, hash_name: &str, key: &str, value: &str) {
    let set_result: redis::RedisResult<()> = redis.hset_nx(hash_name, key, value);
    if let Err(_) = set_result {
        warn!("Cannot set {} in Redis, ignoring", key);
    }
}

fn latest_buildpack_release(http: &HTTPClient, maybe_redis: &Option<redis::Connection>) -> String {
    if let Some(ref redis) = *maybe_redis {
        if let Ok(buildpack_release) = redis.get("latest_buildpack_release") {
            buildpack_release
        } else {
            let buildpack_release = download_latest_buildpack_release(http);
            redis_cache_value(redis, "latest_buildpack_release", &buildpack_release[..])
        }
    } else {
        download_latest_buildpack_release(http)
    }
}

fn cached_version_from_buildpack_release(buildpack_release: &str,
                                         maybe_redis: &Option<redis::Connection>)
                                         -> Option<String> {
    if let Some(ref redis) = *maybe_redis {
        if let Ok(version) = redis.hget("bundler_version", buildpack_release) {
            Some(version)
        } else {
            None
        }
    } else {
        None
    }
}

fn bundler_version_from_ruby_buildpack(http: &HTTPClient,
                                       maybe_redis: Option<redis::Connection>)
                                       -> Option<String> {
    let buildpack_release = latest_buildpack_release(&http, &maybe_redis);
    if let Some(cached_version) = cached_version_from_buildpack_release(&buildpack_release[..],
                                                                        &maybe_redis) {
        return Some(cached_version);
    }
    let ruby_langpack_url = format!("https://raw.githubusercontent.com/\
                                     heroku/heroku-buildpack-ruby/{}/lib/language_pack/\
                                     ruby.rb",
                                    buildpack_release);
    let ruby_file = &download_url(&http, &ruby_langpack_url[..])
                         .expect("Could not download ruby.rb!")[..];
    let regex = Regex::new(r#"BUNDLER_VERSION += "(.+?)""#).expect("Invalid regular expression!");
    if regex.is_match(ruby_file) {
        let captures = regex.captures(ruby_file).expect("Could not match?!");
        let version = captures.at(1).expect("Capture not found?!");
        if let Some(redis) = maybe_redis {
            redis_cache_hash_value(&redis, "bundler_version", &buildpack_release[..], version);
            redis_cache_value(&redis, "latest_supported_bundler_version", version);
        }
        Some(version.to_owned())
    } else {
        None
    }
}

fn is_bundler_upgraded(http: &HTTPClient,
                       redis_result: redis::RedisResult<redis::Connection>)
                       -> bool {
    let bundler_version_result = if let Ok(redis) = redis_result {
        match redis.get("latest_supported_bundler_version") {
            Ok(version) => Some(version),
            _ => bundler_version_from_ruby_buildpack(&http, Some(redis)),
        }
    } else {
        bundler_version_from_ruby_buildpack(&http, None)
    };
    if let Some(buildpack_bundler_version_str) = bundler_version_result {
        let min_version = Version::parse(MIN_BUNDLER_VERSION)
                              .expect("Could not parse min version!");
        let new_version = Version::parse(&buildpack_bundler_version_str[..])
                              .expect("Could not parse new version!");
        min_version < new_version
    } else {
        false
    }
}

fn determine_content_type(req: &Request) -> ContentType {
    if let Some(accept) = req.headers.get::<Accept>() {
        let mut use_json = false;
        for qitem in &accept.0 {
            let qitem_str = &format!("{}", qitem)[..];
            if qitem_str == "application/json" {
                use_json = true;
                break;
            }
        }

        if use_json {
            ContentType::json()
        } else {
            ContentType::html()
        }
    } else {
        ContentType::html()
    }
}

fn result_to_json(result: bool) -> String {
    let mut map = HashMap::new();
    map.insert("result".to_owned(), result);
    serde_json::to_string(&map).expect("Could not serialize result!")
}

fn result_to_html(result: bool) -> String {
    let result_str = if result {
        r#"<p class="yes"><i class="emoji"></i>Yes</p>"#
    } else {
        r#"<p class="no"><i class="emoji"></i>No</p>"#
    };
    match File::open("index.html") {
        Ok(mut html_file) => {
            let mut html = String::new();
            html_file.read_to_string(&mut html).expect("Could not read HTML file!");
            html.replace("{{ is_bundler_upgraded }}", result_str)
                .replace("{{ MIN_BUNDLER_VERSION }}", MIN_BUNDLER_VERSION)
        }
        Err(error) => format!("HTML NOT FOUND: {}", error),
    }
}

fn server_port() -> String {
    match env::var("PORT") {
        Ok(port) => port,
        _ => format!("{}", DEFAULT_SERVER_PORT),
    }
}

fn connect_to_redis() -> redis::RedisResult<redis::Connection> {
    if let Ok(redis_url) = env::var("REDIS_URL") {
        let client = redis::Client::open(&redis_url[..]).expect("Cannot connect to Redis");
        client.get_connection()
    } else {
        Err(redis::RedisError::from(io::Error::new(io::ErrorKind::Other, "REDIS_URL not found")))
    }
}

fn request_handler(req: Request, mut res: Response) {
    if let AbsolutePath(ref path) = req.uri {
        if *path == "/" {
            if req.method == Get {
                let http = HTTPClient::new();
                let redis = connect_to_redis();
                let result = is_bundler_upgraded(&http, redis);
                let content_type = determine_content_type(&req);
                let data = match &format!("{}", content_type)[..] {
                    "application/json; charset=utf-8" => result_to_json(result),
                    _ => result_to_html(result),
                };
                res.headers_mut().set(content_type);
                res.send(data.as_bytes()).expect("Could not set response body!");
            } else {
                *res.status_mut() = hyper::status::StatusCode::MethodNotAllowed;
            }
        } else {
            *res.status_mut() = hyper::NotFound;
        }
    } else {
        *res.status_mut() = hyper::BadRequest;
    }
}

fn main() {
    env_logger::init().expect("Could not initialize env_logger!");
    let server = Server::http(&format!("0.0.0.0:{}", server_port())[..])
                     .expect("Could not create server!");
    server.handle(request_handler).expect("Could not set up HTTP request handler!");
}
