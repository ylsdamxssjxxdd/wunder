"use strict";

import { createSlide } from "./utils.js";

// 第 4 页：工具体系总览，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="工具体系">
        <div class="slide-meta">
          <span class="section-tag">第2节 工具体系</span>
          <div class="section-map">
            <a class="section-chip active" href="#4">总览</a>
            <a class="section-chip" href="#5">内置</a>
            <a class="section-chip" href="#6">MCP</a>
            <a class="section-chip" href="#7">Skills</a>
            <a class="section-chip" href="#8">知识库</a>
            <a class="section-chip" href="#9">自建</a>
            <a class="section-chip" href="#10">共享</a>
          </div>
        </div>
        <h2>六类工具构成能力地图</h2>
        <p class="section-lead">能力拆分后，才能被治理、复用与共享</p>
        <div class="grid three">
          <div class="card">
            <h3>内置工具</h3>
            <p>文件与命令等基础动作</p>
          </div>
          <div class="card">
            <h3>MCP 工具</h3>
            <p>外部系统与平台能力接入</p>
          </div>
          <div class="card">
            <h3>Skills 工具</h3>
            <p>把经验固化成标准流程</p>
          </div>
          <div class="card">
            <h3>知识库工具</h3>
            <p>资料可检索、可追溯</p>
          </div>
          <div class="card">
            <h3>自建工具</h3>
            <p>个人专属能力包</p>
          </div>
          <div class="card">
            <h3>共享工具</h3>
            <p>团队复用能力池</p>
          </div>
        </div>
        <div class="note">
          <strong>统一治理：</strong>统一清单、白名单管控、可组合使用
        </div>
      </section>
  `);
}
