import { normalizeApiBase } from "./utils.js?v=20251229-02";

// 获取当前规范化后的 /wunder 根路径
export const getWunderBase = () => {
  if (typeof window !== "undefined" && window.location?.origin) {
    return normalizeApiBase(`${window.location.origin}/wunder`);
  }
  return normalizeApiBase("/wunder");
};




