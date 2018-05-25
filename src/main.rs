// Copyright (C) 2016, 2017 Mark Lee

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
extern crate futures;
extern crate hyper;
#[macro_use]
extern crate log;
extern crate redis;
extern crate regex;
extern crate reqwest;
extern crate rquery;
extern crate semver;
extern crate serde;
extern crate serde_json;

use hyper::header::{Accept, ContentType};
use hyper::server::{Http, Request, Response, Service};
use hyper::{Get, StatusCode};
use redis::{Commands, RedisError, RedisResult};
use regex::Regex;
use rquery::Document;
use semver::Version;
use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io;
use std::io::Read;

const DEFAULT_SERVER_PORT: u32 = 9000;
const DEFAULT_MIN_BUNDLER_VERSION: &str = "1.12.0";
const RUBY_LANGPACK_RELEASES_URL: &str =
    "https://github.com/heroku/heroku-buildpack-ruby/releases.atom";

fn download_url(url: &str) -> io::Result<String> {
    let mut resp = reqwest::get(url).expect("Could not send HTTP request");
    let mut body = String::new();
    match resp.read_to_string(&mut body) {
        Err(error) => Err(error),
        _ => Ok(body),
    }
}

fn download_latest_buildpack_release() -> String {
    let xml = download_url(RUBY_LANGPACK_RELEASES_URL).expect("Could not download Atom releases!");
    let atom_doc = Document::new_from_xml_string(&xml[..]).expect("Could not parse Atom releases!");
    atom_doc
        .select("entry id")
        .expect("Could not find latest buildpack tag!")
        .text()
        .rsplitn(2, '/')
        .next()
        .expect("No slashes in the GitHub release ID!")
        .to_string()
}

fn redis_cache_value(redis: &redis::Connection, key: &str, value: &str) -> String {
    let set_result: RedisResult<()> = redis.set_ex(key, value, 3600);
    if set_result.is_err() {
        warn!("Cannot set {} in Redis, ignoring", key);
    }
    value.to_owned()
}

fn redis_cache_hash_value(redis: &redis::Connection, hash_name: &str, key: &str, value: &str) {
    let set_result: RedisResult<()> = redis.hset_nx(hash_name, key, value);
    if set_result.is_err() {
        warn!("Cannot set {} in Redis, ignoring", key);
    }
}

fn latest_buildpack_release(redis_result: &RedisResult<redis::Connection>) -> String {
    if let Ok(ref redis) = *redis_result {
        if let Ok(buildpack_release) = redis.get("latest_buildpack_release") {
            buildpack_release
        } else {
            let buildpack_release = download_latest_buildpack_release();
            redis_cache_value(redis, "latest_buildpack_release", &buildpack_release[..])
        }
    } else {
        download_latest_buildpack_release()
    }
}

fn cached_version_from_buildpack_release(
    buildpack_release: &str,
    redis_result: &RedisResult<redis::Connection>,
) -> Option<String> {
    if let Ok(ref redis) = *redis_result {
        if let Ok(version) = redis.hget("bundler_version", buildpack_release) {
            Some(version)
        } else {
            None
        }
    } else {
        None
    }
}

fn bundler_version_from_ruby_buildpack(
    redis_result: &RedisResult<redis::Connection>,
) -> Option<String> {
    let buildpack_release = latest_buildpack_release(redis_result);
    if let Some(cached_version) =
        cached_version_from_buildpack_release(&buildpack_release[..], redis_result)
    {
        return Some(cached_version);
    }
    let ruby_langpack_url = format!(
        "https://raw.githubusercontent.com/\
         heroku/heroku-buildpack-ruby/{}/lib/language_pack/\
         ruby.rb",
        buildpack_release
    );
    let ruby_file = &download_url(&ruby_langpack_url[..]).expect("Could not download ruby.rb!")[..];
    let regex = Regex::new(r#"BUNDLER_VERSION += "(.+?)""#).expect("Invalid regular expression!");
    if regex.is_match(ruby_file) {
        let captures = regex.captures(ruby_file).expect("Could not match?!");
        let version_match = captures.get(1).expect("Capture not found?!");
        let version = version_match.as_str();
        if let Ok(ref redis) = *redis_result {
            redis_cache_hash_value(redis, "bundler_version", &buildpack_release[..], version);
        }
        Some(version.to_owned())
    } else {
        None
    }
}

fn is_bundler_upgraded(redis_result: &RedisResult<redis::Connection>) -> bool {
    let bundler_version_result = bundler_version_from_ruby_buildpack(redis_result);
    if let Some(buildpack_bundler_version_str) = bundler_version_result {
        let min_version =
            Version::parse(&min_bundler_version()[..]).expect("Could not parse min version!");
        let new_version = Version::parse(&buildpack_bundler_version_str[..])
            .expect("Could not parse new version!");
        min_version < new_version
    } else {
        false
    }
}

fn determine_content_type(headers: &hyper::Headers) -> ContentType {
    use std::ops::Deref;
    if headers.has::<Accept>() {
        let accept = headers
            .get::<Accept>()
            .expect("Accept header not found?!")
            .deref();
        let use_json = accept
            .into_iter()
            .any(|qitem| qitem.item == "application/json");

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
            html_file
                .read_to_string(&mut html)
                .expect("Could not read HTML file!");
            html.replace("{{ is_bundler_upgraded }}", result_str)
                .replace("{{ MIN_BUNDLER_VERSION }}", &min_bundler_version()[..])
        }
        Err(error) => format!("HTML NOT FOUND: {}", error),
    }
}

fn min_bundler_version() -> String {
    match env::var("MIN_BUNDLER_VERSION") {
        Ok(version) => version,
        _ => String::from(DEFAULT_MIN_BUNDLER_VERSION),
    }
}

fn server_port() -> String {
    match env::var("PORT") {
        Ok(port) => port,
        _ => format!("{}", DEFAULT_SERVER_PORT),
    }
}

fn connect_to_redis() -> RedisResult<redis::Connection> {
    if let Ok(redis_url) = env::var("REDIS_URL") {
        let client = redis::Client::open(&redis_url[..]).expect("Cannot connect to Redis");
        client.get_connection()
    } else {
        Err(RedisError::from(io::Error::new(
            io::ErrorKind::Other,
            "REDIS_URL not found",
        )))
    }
}

struct HBBUYServer;

impl Service for HBBUYServer {
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;
    type Future = futures::future::FutureResult<Self::Response, Self::Error>;

    fn call(&self, req: Request) -> Self::Future {
        let response = match (req.uri().path(), req.method()) {
            ("/", &Get) => {
                let redis = connect_to_redis();
                let result = is_bundler_upgraded(&redis);
                let content_type = determine_content_type(req.headers());
                let data = match &format!("{}", content_type)[..] {
                    "application/json" => result_to_json(result),
                    _ => result_to_html(result),
                };

                Response::new().with_header(content_type).with_body(data)
            }
            (_, &Get) => Response::new().with_status(StatusCode::NotFound),
            (_, _) => Response::new().with_status(StatusCode::MethodNotAllowed),
        };

        futures::future::ok(response)
    }
}

fn main() {
    env_logger::try_init().expect("Could not initialize env_logger!");
    let addr = format!("0.0.0.0:{}", server_port())
        .parse()
        .expect("Could not parse address/port");
    let server = Http::new()
        .bind(&addr, || Ok(HBBUYServer))
        .expect("Could not create server!");
    server
        .run()
        .expect("Could not set up HTTP request handler!");
}
