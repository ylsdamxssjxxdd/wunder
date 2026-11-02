"""Keep only the newest CI releases, so an offline runner's disk stays bounded.

留存策略与在线流水线里的 KEEP_CI_RELEASES 一致：只按标签前缀 ci- 排序删除，
手工发布的 v* 标签绝不触碰。删除 Release 时连带删标签，失败只记录不中断。
"""

import json
import os
from urllib.error import HTTPError
from urllib.parse import quote
from urllib.request import Request, urlopen


def main():
    server = os.environ.get('CI_SERVER_URL', 'http://gitea:3000').rstrip('/')
    repository = os.environ['CI_REPOSITORY']
    token = os.environ['CI_TOKEN']
    keep = int(os.environ.get('KEEP_CI_RELEASES', '5'))
    if keep < 1:
        raise ValueError('KEEP_CI_RELEASES must be at least 1')
    prefix = '/api/v1/repos/' + quote(repository, safe='/')

    def api(method, path):
        request = Request(server + path, method=method,
                          headers={'Authorization': 'token ' + token})
        with urlopen(request, timeout=60) as response:
            content = response.read()
            return json.loads(content) if content else []

    releases = []
    page = 1
    while True:
        batch = api('GET', prefix + '/releases?limit=50&page=' + str(page))
        releases.extend(batch)
        if len(batch) < 50:
            break
        page += 1

    ci = sorted((release for release in releases
                 if release['tag_name'].startswith('ci-')),
                key=lambda release: (release['created_at'], release['id']), reverse=True)
    for release in ci[keep:]:
        try:
            api('DELETE', prefix + '/releases/' + str(release['id']))
        except HTTPError as error:
            print('prune failed for %s: HTTP %d' % (release['tag_name'], error.code))
            continue
        try:
            api('DELETE', prefix + '/tags/' + quote(release['tag_name'], safe=''))
        except HTTPError as error:
            print('tag %s is already gone: HTTP %d' % (release['tag_name'], error.code))
        print('pruned release ' + release['tag_name'])
    print('kept ci releases: ' + ', '.join(release['tag_name'] for release in ci[:keep]))


if __name__ == '__main__':
    main()
