import { elements } from "./elements.js?v=20260104-09";
import { formatToolSchema } from "./utils.js?v=20251229-02";
import { t } from "./i18n.js?v=20260104-09";

// 打开工具详情弹窗：内置工具与 MCP 工具共用展示逻辑
export const openToolDetailModal = (payload) => {
  const title = String(payload?.title || payload?.name || "").trim();
  elements.toolDetailTitle.textContent = title || t("tool.detail.title");
  elements.toolDetailMeta.textContent = payload?.meta || "";
  const description = String(payload?.description || "").trim();
  elements.toolDetailDesc.textContent = description || t("tool.detail.noDescription");
  elements.toolDetailSchema.textContent = formatToolSchema(payload?.schema);
  elements.toolDetailModal.classList.add("active");
};

// 关闭工具详情弹窗，保留内容便于快速切换查看
export const closeToolDetailModal = () => {
  elements.toolDetailModal.classList.remove("active");
};

// 初始化工具详情弹窗交互
export const initToolDetailModal = () => {
  elements.toolDetailClose.addEventListener("click", closeToolDetailModal);
  elements.toolDetailCloseBtn.addEventListener("click", closeToolDetailModal);
  elements.toolDetailModal.addEventListener("click", (event) => {
    if (event.target === elements.toolDetailModal) {
      closeToolDetailModal();
    }
  });
};




