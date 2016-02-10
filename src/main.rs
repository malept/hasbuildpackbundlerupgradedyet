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

use hyper::Client;
use regex::Regex;
use rquery::Document;
use semver::Version;
use std::io::{Error, Read};

const OLD_BUNDLER_VERSION: &'static str = "1.9.7";
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
        let old_version = Version::parse(OLD_BUNDLER_VERSION)
                              .expect("Could not parse old version!");
        let new_version = Version::parse(&buildpack_bundler_version_str[..])
                              .expect("Could not parse new version!");
        old_version >= new_version
    } else {
        false
    }
}

fn main() {
    let client = Client::new();
    if is_bundler_upgraded(&client) {
        println!("NO");
    } else {
        println!("YES");
    }
}
