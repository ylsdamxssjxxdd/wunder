## 1. 已下载待应用，暂存包（desktop，未校验），运行版本 5

Rule broken: 暂存区里的待应用包一定是下载时通过清单哈希校验的包。
Why: 下载完成没有做清单哈希校验，未校验的包直接进入暂存区.
In the model, one step leads into this bug: "下载完成并校验清单哈希". The steps below are the shortest path that TLC found.

0. Start: 空闲，可检查更新，无暂存包，运行版本 5
1. 点击检查更新. Now: 正在检查更新，无暂存包，运行版本 5
2. 源上有更新的版本. Now: 发现新版本，无暂存包，运行版本 5
3. 点击下载更新. Now: 正在下载更新包，无暂存包，运行版本 5
4. 下载完成并校验清单哈希. Now: 已下载待应用，暂存包（desktop，未校验），运行版本 5

With the fix, step 4 (下载完成并校验清单哈希) takes you to: 已下载待应用，暂存包（desktop，已校验），运行版本 5.
