// Copyright (C) 2016, 2017, 2018, 2025 Mark Lee

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

use axum::response::{Html, IntoResponse, Json, Response};
use axum::{Router, routing::get};

use axum::http::header::{ACCEPT, HeaderMap};
use log::warn;
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
const DEFAULT_MIN_BUNDLER_VERSION: &str = "1.16.0";
const RUBY_LANGPACK_RELEASES_URL: &str =
    "https://github.com/heroku/heroku-buildpack-ruby/releases.atom";

async fn download_url(url: &str) -> reqwest::Result<String> {
    reqwest::get(url)
        .await
        .expect("Could not send HTTP request")
        .text()
        .await
}

async fn download_latest_buildpack_release() -> String {
    let xml = download_url(RUBY_LANGPACK_RELEASES_URL)
        .await
        .expect("Could not download Atom releases!");
    let atom_doc = Document::new_from_xml_string(&xml[..]).expect("Could not parse Atom releases!");
    atom_doc
        .select("entry id")
        .expect("Could not find latest buildpack tag!")
        .text()
        .rsplit_once('/')
        .map(|x| x.1)
        .expect("No slashes in the GitHub release ID!")
        .to_string()
}

fn redis_cache_value(redis: &mut redis::Connection, key: &str, value: &str) -> String {
    let set_result: RedisResult<()> = redis.set_ex(key, value, 3600);
    if set_result.is_err() {
        warn!("Cannot set {key} in Redis, ignoring");
    }
    value.to_owned()
}

fn redis_cache_hash_value(redis: &mut redis::Connection, hash_name: &str, key: &str, value: &str) {
    let set_result: RedisResult<()> = redis.hset_nx(hash_name, key, value);
    if set_result.is_err() {
        warn!("Cannot set {key} in Redis, ignoring");
    }
}

async fn latest_buildpack_release(redis_result: &mut RedisResult<redis::Connection>) -> String {
    if let Ok(ref mut redis) = *redis_result {
        if let Ok(buildpack_release) = redis.get("latest_buildpack_release") {
            buildpack_release
        } else {
            let buildpack_release = download_latest_buildpack_release().await;
            redis_cache_value(redis, "latest_buildpack_release", &buildpack_release[..])
        }
    } else {
        download_latest_buildpack_release().await
    }
}

fn cached_version_from_buildpack_release(
    buildpack_release: &str,
    redis_result: &mut RedisResult<redis::Connection>,
) -> Option<String> {
    if let Ok(ref mut redis) = *redis_result {
        redis.hget("bundler_version", buildpack_release).ok()
    } else {
        None
    }
}

async fn bundler_version_from_ruby_buildpack(
    redis_result: &mut RedisResult<redis::Connection>,
) -> Option<String> {
    let buildpack_release = latest_buildpack_release(redis_result).await;
    if let Some(cached_version) =
        cached_version_from_buildpack_release(&buildpack_release[..], redis_result)
    {
        return Some(cached_version);
    }
    let ruby_langpack_url = format!(
        "https://raw.githubusercontent.com/\
         heroku/heroku-buildpack-ruby/{buildpack_release}/lib/language_pack/\
         ruby.rb"
    );
    let ruby_file = &download_url(&ruby_langpack_url[..])
        .await
        .expect("Could not download ruby.rb!")[..];
    let regex = Regex::new(r#"BUNDLER_VERSION += "(.+?)""#).expect("Invalid regular expression!");
    if regex.is_match(ruby_file) {
        let captures = regex.captures(ruby_file).expect("Could not match?!");
        let version_match = captures.get(1).expect("Capture not found?!");
        let version = version_match.as_str();
        if let Ok(ref mut redis) = *redis_result {
            redis_cache_hash_value(redis, "bundler_version", &buildpack_release[..], version);
        }
        Some(version.to_owned())
    } else {
        None
    }
}

async fn is_bundler_upgraded(redis_result: &mut RedisResult<redis::Connection>) -> bool {
    let bundler_version_result = bundler_version_from_ruby_buildpack(redis_result).await;
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

fn determine_content_type(headers: &HeaderMap) -> &'static str {
    if headers.contains_key(ACCEPT) {
        let accept_value = headers
            .get(ACCEPT)
            .expect("Accept header not found?!")
            .to_str()
            .expect("Could not serialize Accept value?!");

        if accept_value.contains("application/json") {
            "application/json"
        } else {
            "text/html"
        }
    } else {
        "text/html"
    }
}

fn result_to_json(result: bool) -> Json<HashMap<String, bool>> {
    let mut map = HashMap::new();
    map.insert("result".to_owned(), result);
    Json(map)
}

fn result_to_html(result: bool) -> Html<String> {
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
            Html(
                html.replace("{{ is_bundler_upgraded }}", result_str)
                    .replace("{{ MIN_BUNDLER_VERSION }}", &min_bundler_version()[..]),
            )
        }
        Err(error) => Html(format!("HTML NOT FOUND: {error}")),
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
        _ => format!("{DEFAULT_SERVER_PORT}"),
    }
}

fn connect_to_redis() -> RedisResult<redis::Connection> {
    if let Ok(redis_url) = env::var("REDIS_URL") {
        let client = redis::Client::open(&redis_url[..]).expect("Cannot connect to Redis");
        client.get_connection()
    } else {
        Err(RedisError::from(io::Error::other("REDIS_URL not found")))
    }
}

async fn response(headers: HeaderMap) -> Response {
    let mut redis = connect_to_redis();
    let result = is_bundler_upgraded(&mut redis).await;
    match determine_content_type(&headers) {
        "application/json" => result_to_json(result).into_response(),
        _ => result_to_html(result).into_response(),
    }
}

#[tokio::main]
async fn main() {
    env_logger::try_init().expect("Could not initialize env_logger!");
    let app = Router::new().route("/", get(response));

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", server_port()))
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();
}
