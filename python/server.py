#!/usr/bin/env python3
# Copyright (C) 2016, 2025 Mark Lee
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
"""
Python HTTP server for "has buildpack Bundler upgraded yet".
"""

import os
import re
from pathlib import Path

import aiohttp
from accept_types import get_best_match  # type: ignore[import-untyped]
from lxml import etree
from redis.asyncio import Redis
from semver import Version
from starlette.applications import Starlette
from starlette.requests import Request
from starlette.responses import HTMLResponse, JSONResponse, Response
from starlette.routing import Route
from starlette.status import HTTP_406_NOT_ACCEPTABLE

INDEX_HTML = (Path(__file__) / ".." / ".." / "index.html").resolve()
MIN_BUNDLER_VERSION = os.environ.get("MIN_BUNDLER_VERSION", "1.16.0")
REGEX = re.compile(r'BUNDLER_VERSION += "(.+?)"')
RUBY_LANGPACK_RELEASES_URL = """\
https://github.com/heroku/heroku-buildpack-ruby/releases.atom\
"""
RUBY_LANGPACK_URL = """\
https://raw.githubusercontent.com/heroku/heroku-buildpack-ruby/{}/\
lib/language_pack/ruby.rb\
"""
TYPE_JSON = "application/json"
TYPE_HTML = "text/html"


class Buildpack:
    """Represents a Ruby buildpack."""

    redis: Redis | None
    http: aiohttp.ClientSession

    def __init__(self, session: aiohttp.ClientSession, redis: Redis | None) -> None:
        """Creates a new Buildpack instance."""
        self.redis = redis
        self.http = session

    async def http_get(self, url: str) -> str:
        """Wrapper for getting the response body from an HTTP GET request."""
        response = await self.http.get(url)
        return await response.text()

    async def latest_release(self) -> str:
        """
        Determines the latest tagged version of the Ruby buildpack, which is
        cached in Redis if provided.
        """
        releases = await self.http_get(RUBY_LANGPACK_RELEASES_URL)
        xml = etree.fromstring(releases.encode("utf-8"))
        nsmap = {"atom": xml.nsmap[None]}
        release: str = xml.xpath("atom:entry/atom:id/text()", namespaces=nsmap)[
            0
        ].rsplit(
            "/",
            1,
        )[1]
        if self.redis:
            await self.redis.setex("latest_buildpack_release", 3600, release)
        return release

    async def bundler_version(self) -> Version | None:
        """
        Determines the Bundler version required for the latest buildpack
        release, expressed as a semver Version.
        """
        buildpack_release: str | None = None
        if self.redis:
            buildpack_release = await self.redis.get("latest_buildpack_release")
        if not buildpack_release:
            buildpack_release = await self.latest_release()

        ruby_file = await self.http_get(RUBY_LANGPACK_URL.format(buildpack_release))
        match = REGEX.search(ruby_file)
        if match:
            bundler_version = match.group(1)
            if self.redis:
                await self.redis.hsetnx(
                    "bundler_version",
                    buildpack_release,
                    bundler_version,
                )  # type: ignore[misc]
            return Version.parse(bundler_version)
        return None


async def is_bundler_upgraded(
    session: aiohttp.ClientSession, redis: Redis | None
) -> bool:
    """
    Determines if the version of Bundler used by the Ruby buildpack is greater
    than the minimum version of Bundler.
    """
    buildpack = Buildpack(session, redis)
    version = await buildpack.bundler_version()
    if version is None:
        return False
    return version.match(f">={MIN_BUNDLER_VERSION}")


def html_result(result: bool) -> str:
    """Render a boolean as emoji-fied HTML."""
    if result:
        return '<p class="yes"><i class="emoji"></i>Yes</p>'
    return '<p class="no"><i class="emoji"></i>No</p>'


def to_html(result: bool) -> str:
    """Render the result of ``is_bundler_upgraded`` as HTML."""
    try:
        with INDEX_HTML.open() as f:
            index = f.read()
        index = index.replace("{{ is_bundler_upgraded }}", html_result(result))
        return index.replace("{{ MIN_BUNDLER_VERSION }}", MIN_BUNDLER_VERSION)
    except OSError as e:
        return f"HTML NOT FOUND: {e}"


async def upgraded(request: Request) -> Response:
    """
    HTTP handler that determines if the version of Bundler in the latest Ruby
    buildpack is greater than or equal to the minimum Bundler version.
    """
    redis_url = os.environ.get("REDIS_URL")
    redis = Redis.from_url(redis_url) if redis_url else None
    async with aiohttp.ClientSession() as session:
        result = await is_bundler_upgraded(session, redis)

    content_type = get_best_match(request.headers.get("accept"), [TYPE_JSON, TYPE_HTML])
    if content_type == TYPE_JSON:
        return JSONResponse({"result": result})
    if content_type == TYPE_HTML:
        return HTMLResponse(to_html(result))
    return Response("JSON or HTML output only", status_code=HTTP_406_NOT_ACCEPTABLE)


app = Starlette(routes=[Route("/", upgraded)])
