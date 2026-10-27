"""Publish artifacts with the short-lived Gitea workflow token."""

import http.client
import json
import mimetypes
import os
from pathlib import Path
import sys
import time
from urllib.error import HTTPError
from urllib.parse import quote, urlsplit
from urllib.request import Request, urlopen


def main():
    directory = Path(sys.argv[1])
    server = os.environ.get("CI_SERVER_URL", "http://gitea:3000").rstrip("/")
    repository = os.environ["CI_REPOSITORY"]
    commit = os.environ["CI_COMMIT"]
    token = os.environ["CI_TOKEN"]
    run = os.environ.get("GITHUB_RUN_NUMBER", "manual")
    tag = "ci-" + commit[:12] + "-" + run
    prefix = "/api/v1/repos/" + quote(repository, safe="/")

    def api(method, path, body=None):
        data = None if body is None else json.dumps(body).encode()
        request = Request(server + path, data=data, method=method, headers={
            "Authorization": "token " + token, "Content-Type": "application/json"})
        with urlopen(request, timeout=60) as response:
            content = response.read()
            return json.loads(content) if content else None

    release_path = prefix + "/releases/tags/" + quote(tag, safe="")
    try:
        release = api("GET", release_path)
    except HTTPError as error:
        if error.code != 404:
            raise
        try:
            release = api("POST", prefix + "/releases", {
                "tag_name": tag, "target_commitish": commit,
                "name": "Offline Linux build " + run + " (" + commit[:12] + ")",
                "body": "Automatic offline ARM64/amd64 desktop and CLI build. "
                        "Architecture jobs attach their outputs independently; check Actions for completion.",
                "draft": False, "prerelease": True})
        except HTTPError as conflict:
            if conflict.code not in (409, 422):
                raise
            release = api("GET", release_path)
    release_id = release["id"]
    existing = api("GET", prefix + "/releases/" + str(release_id) + "/assets")
    for artifact in sorted(directory.iterdir()):
        if not artifact.is_file():
            continue
        for asset in existing:
            if asset["name"] == artifact.name:
                api("DELETE", prefix + "/releases/" + str(release_id) + "/assets/" + str(asset["id"]))
        parsed = urlsplit(server)
        boundary = "offline-gitea-upload"
        header = ("--" + boundary + "\r\nContent-Disposition: form-data; name=\"attachment\"; "
                  "filename=\"" + artifact.name + "\"\r\nContent-Type: "
                  + (mimetypes.guess_type(artifact.name)[0] or "application/octet-stream")
                  + "\r\n\r\n").encode()
        trailer = ("\r\n--" + boundary + "--\r\n").encode()
        connection_class = http.client.HTTPSConnection if parsed.scheme == "https" else http.client.HTTPConnection
        for attempt in range(3):
            connection = connection_class(parsed.hostname, parsed.port, timeout=300)
            try:
                path = parsed.path.rstrip("/") + prefix + "/releases/" + str(release_id) + "/assets"
                connection.putrequest("POST", path)
                connection.putheader("Authorization", "token " + token)
                connection.putheader("Content-Type", "multipart/form-data; boundary=" + boundary)
                connection.putheader("Content-Length", str(len(header) + artifact.stat().st_size + len(trailer)))
                connection.endheaders()
                connection.send(header)
                with artifact.open("rb") as stream:
                    while True:
                        chunk = stream.read(1024 * 1024)
                        if not chunk:
                            break
                        connection.send(chunk)
                connection.send(trailer)
                response = connection.getresponse()
                response.read()
                if response.status not in (200, 201):
                    raise RuntimeError("artifact upload failed with HTTP " + str(response.status))
                print("Published " + artifact.name)
                break
            except (OSError, http.client.HTTPException):
                if attempt == 2:
                    raise
                time.sleep(2 ** attempt)
            finally:
                connection.close()
    print("Release: " + str(release.get("html_url", tag)))


if __name__ == "__main__":
    main()
