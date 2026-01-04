"use strict";

import { createSlide } from "./utils.js";

// 第 7 页：MCP 工具，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="MCP 工具">
        <div class="slide-meta">
          <span class="section-tag">第2节 工具体系</span>
          <div class="section-map">
            <a class="section-chip" href="#5">总览</a>
            <a class="section-chip" href="#6">内置</a>
            <a class="section-chip active" href="#7">MCP</a>
            <a class="section-chip" href="#8">Skills</a>
            <a class="section-chip" href="#9">知识库</a>
            <a class="section-chip" href="#10">自建</a>
            <a class="section-chip" href="#11">共享</a>
          </div>
        </div>
        <h2>MCP 工具：接入外部系统</h2>
        <p class="section-lead">当内置不够用，就把外部能力接进来</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">是什么</span>
            <ul>
              <li>通过 MCP 协议接入外部服务</li>
              <li>统一以 server@tool 方式调用</li>
              <li>自动加入工具清单管理</li>
            </ul>
            <span class="pill">有什么用</span>
            <ul>
              <li>连接企业系统、搜索、BI 等能力</li>
              <li>形成跨系统的执行链路</li>
            </ul>
            <span class="pill">治理要点</span>
            <p>allow_tools 白名单 + 统一超时控制</p>
          </div>
          <div class="card media-panel stack">
            <h3>图片占位</h3>
            <p>建议：MCP 接入拓扑或外部服务示意</p>
            <span class="tag">assets/tool-mcp.png</span>
          </div>
        </div>
      </section>
  `);
}
