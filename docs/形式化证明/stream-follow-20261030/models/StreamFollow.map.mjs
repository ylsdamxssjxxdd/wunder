export const readerActions = ['ReaderScrollsUpOuter', 'ReaderBackToTailOuter', 'ReaderScrollsUpInner', 'ReaderBackToTailInner'];
export function state(v) {
  const lane = (name, grown, followed, tail, pending) =>
    `${name}：最新${grown}段，视口停在${followed}${tail ? '，跟随中' : '，读者在回看'}${pending ? '，跟随待触发' : ''}`;
  return `${lane('外层时间线', v.oGrown, v.oFollowed, v.oFollowTail, v.oPending)}<br/>${lane('工作过程列表', v.iGrown, v.iFollowed, v.iFollowTail, v.iPending)}`;
}
export function step(action) {
  return {
    OuterPublishesFrame: '外层发布新一帧，重武装跟随',
    OuterContentSettles: '外层内容变高，重武装跟随',
    OuterFollowPass: '外层跟随到最新',
    InnerNewEntry: '过程区新增条目，重武装跟随',
    InnerEntryGrows: '过程区条目流式变高（无新增信号）',
    InnerFollowPass: '过程区跟随到最新',
    ReaderScrollsUpOuter: '读者上滚外层，停止跟随',
    ReaderBackToTailOuter: '读者回到外层底部，恢复跟随',
    ReaderScrollsUpInner: '读者上滚过程区，停止跟随',
    ReaderBackToTailInner: '读者回到过程区底部，恢复跟随',
  }[action] || action;
}
export function cause() { return '流式变高未重武装跟随'; }
export function group(v) { return v.iFollowed < v.iGrown ? '过程区落后' : '视口已跟上'; }
