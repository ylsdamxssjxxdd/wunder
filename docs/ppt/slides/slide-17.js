"use strict";

(() => {

const { createSlide, registerSlide } = window.WunderPpt;

function buildSlide() {
  return createSlide(`
<section class="slide" data-title="安全边界">
  <div class="slide-meta">
    <span class="section-tag">第5节 治理与稳定性</span>
    <div class="section-map">
      <a class="section-chip" href="#16">组织治理</a>
      <a class="section-chip active" href="#17">安全边界</a>
      <a class="section-chip" href="#18">观测评测</a>
    </div>
  </div>
  <h2>安全边界：白名单、拒绝规则、沙盒执行</h2>
  <p class="section-lead">执行能力越强，越要清晰定义可执行范围</p>
  <div class="grid two">
    <div class="card stack">
      <span class="pill">执行边界</span>
      <p>allow_paths / allow_commands / deny_globs 联合约束。</p>
      <span class="pill">风险隔离</span>
      <p>高风险命令与脚本可下沉到 sandbox 服务执行。</p>
      <span class="pill">鉴权机制</span>
      <p>管理员 API Key、用户 Bearer Token、渠道签名/令牌校验。</p>
      <span class="pill">控制面防护</span>
      <p>Gateway 握手、origin 与 trusted proxy 策略共同生效。</p>
    </div>
    <div class="card media-panel is-image stack">
      <img src="assets/security-boundary-sandbox.svg" alt="安全边界与沙盒隔离示意图" />
    </div>
  </div>
</section>
  `);
}

registerSlide(buildSlide);

})();
