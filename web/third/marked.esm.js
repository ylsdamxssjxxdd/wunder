import "./marked.min.js";

const isMarkedApi = (candidate) =>
  Boolean(candidate && typeof candidate.parse === "function" && typeof candidate.Renderer === "function");

const resolveMarkedApi = () => {
  if (isMarkedApi(globalThis.marked)) {
    return globalThis.marked;
  }
  if (isMarkedApi(globalThis.marked?.marked)) {
    return globalThis.marked.marked;
  }
  if (isMarkedApi(globalThis.module?.exports)) {
    return globalThis.module.exports;
  }
  if (isMarkedApi(globalThis.module?.exports?.marked)) {
    return globalThis.module.exports.marked;
  }
  if (isMarkedApi(globalThis.exports)) {
    return globalThis.exports;
  }
  if (isMarkedApi(globalThis.exports?.marked)) {
    return globalThis.exports.marked;
  }
  return null;
};

const markedApi = resolveMarkedApi();
if (!markedApi) {
  throw new Error("Unable to initialize marked from /third/marked.min.js");
}

globalThis.marked = markedApi;

export default markedApi;
export const marked = markedApi;
export const parse = markedApi.parse.bind(markedApi);
export const parseInline = markedApi.parseInline.bind(markedApi);
export const Renderer = markedApi.Renderer;
