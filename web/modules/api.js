import { elements } from "./elements.js?v=20260104-11";
import { normalizeApiBase } from "./utils.js?v=20251229-02";

// 获取当前规范化后的 /wunder 根路径
export const getWunderBase = () => normalizeApiBase(elements.apiBase.value);




