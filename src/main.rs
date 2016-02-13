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

extern crate hyper;
extern crate regex;
extern crate rquery;
extern crate semver;
extern crate serde;
extern crate serde_json;

use hyper::{Client, Get, Server};
use hyper::header::{Accept, ContentType};
use hyper::server::{Request, Response};
use hyper::uri::RequestUri::AbsolutePath;
use regex::Regex;
use rquery::Document;
use semver::Version;
use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::{Error, Read};

const DEFAULT_SERVER_PORT: u32 = 9000;
const MIN_BUNDLER_VERSION: &'static str = "1.11.0";
const RUBY_LANGPACK_RELEASES_URL: &'static str = "https://github.\
                                                  com/heroku/heroku-buildpack-ruby/releases.atom";

fn download_url(client: &Client, url: &str) -> Result<String, Error> {
    let mut resp = client.get(url).send().expect("Could not send HTTP request");
    let mut body = String::new();
    match resp.read_to_string(&mut body) {
        Err(error) => Err(error),
        _ => Ok(body),
    }
}

fn latest_buildpack_release(client: &Client) -> String {
    let xml = download_url(&client, RUBY_LANGPACK_RELEASES_URL)
                  .expect("Could not download Atom releases!");
    let atom_doc = Document::new_from_xml_string(&xml[..]).expect("Could not parse Atom releases!");
    let latest_tag = atom_doc.select("entry title")
                             .expect("Could not find latest buildpack tag!");
    latest_tag.text().clone()
}

fn bundler_version_from_ruby_buildpack(client: &Client) -> Option<String> {
    let ruby_langpack_url = format!("https://raw.githubusercontent.com/\
                                     heroku/heroku-buildpack-ruby/{}/lib/language_pack/\
                                     ruby.rb",
                                    latest_buildpack_release(&client));
    let ruby_file = &download_url(&client, &ruby_langpack_url[..])
                         .expect("Could not download ruby.rb!")[..];
    let regex = Regex::new(r#"BUNDLER_VERSION += "(.+?)""#).expect("Invalid regular expression!");
    if regex.is_match(ruby_file) {
        let captures = regex.captures(ruby_file).expect("Could not match?!");
        Some(captures.at(1).expect("Capture not found?!").to_owned())
    } else {
        None
    }
}

fn is_bundler_upgraded(client: &Client) -> bool {
    if let Some(buildpack_bundler_version_str) = bundler_version_from_ruby_buildpack(&client) {
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
}

fn main() {
    let server = Server::http(&format!("0.0.0.0:{}", server_port())[..])
                     .expect("Could not create server!");
    server.handle(|req: Request, mut res: Response| {
              match req.uri {
                  AbsolutePath(ref path) => {
                      match (&req.method, &path[..]) {
                          (&Get, "/") => {
                              let client = Client::new();
                              let result = is_bundler_upgraded(&client);
                              let content_type = determine_content_type(&req);
                              let data = match &format!("{}", content_type)[..] {
                                  "application/json; charset=utf-8" => result_to_json(result),
                                  _ => result_to_html(result),
                              };
                              res.headers_mut().set(content_type);
                              res.send(data.as_bytes()).expect("Could not set response body!")
                          }
                          _ => {
                              *res.status_mut() = hyper::NotFound;
                          }
                      }
                  }
                  _ => {
                      *res.status_mut() = hyper::BadRequest;
                  }
              }
          })
          .expect("Could not set up HTTP request handler!");
}
