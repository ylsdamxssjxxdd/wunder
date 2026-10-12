# 线程展示读模型 v3 性能基线

本报告记录当前代码的匿名 SQLite 读路径和前端发布构建结果。数据使用 `fixture-*` 标识，不读取工作区真实数据库，也不包含正文、账号或密钥。PostgreSQL 和目标部署浏览器尚未在本机执行，不能据此宣称双库或线上首屏达标。

使用实际 `SqliteStorage` 方法交替采样 30 次，固定聊天 12 行、轨迹 80 行，并完整序列化响应。历史规模为 500,000 条 item：旧读取 p50/p95 为 1,756.53/2,053.80 ms，新聊天为 1.88/3.13 ms，轨迹 p95 为 2.89 ms；新聊天 JSON 约 6.9 KB、轨迹约 25.4 KB，旧响应约 1.36 MB。20,000 和 200,000 条时的新聊天 p95 分别为 2.18 ms、2.36 ms。

SQLite 展示存储测试覆盖用户身份、正文完整回读、文本块幂等/重建 seek、反馈与可见性撤销、延迟块不能覆盖已封存答案；blocking view 类的 8 个独立许可、150 ms 排队预算、2 s 执行预算通过。前端回归覆盖 generation、防旧页覆盖、verified empty、历史游标、乐观绑定、重复/缺段尾流、UTF-16 偏移、有限长尾和独立运行状态；`npm run typecheck`、发布构建及 mock HTTP/WS 浏览器回归通过。

PostgreSQL、真实部署浏览器 60 秒迟到回退、10/50 并发、15 分钟内存稳定性、网络 RTT 和低端设备帧率尚未测量。`frontend/playwright.thread-view-real.config.ts` 提供真实 SQLite/HTTP/WS fixture 入口（2,000 条匿名 fixture item，启动时间有界）；目标环境验收必须记录 click→content paint、LongTask、heap、WS 重连，并分别报告 SQLite 与 PostgreSQL。

真实 fixture 曾成功启动并测得冷导航可见内容约 1.37 秒，但 Playwright webServer 生命周期探测在当前工作区配置下超时；因此该值只作为一次性观测，不作为稳定 p95。
