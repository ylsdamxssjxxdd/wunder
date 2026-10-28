## 1. 相同前缀的新思考<br/>迟到数据处理结束<br/>活动第2轮，展示第2轮<br/>旧思考被新文字覆盖

Rule broken: Completed canonical thoughts remain unchanged; mutators: frontend/src/components/chat/MessageTimelineTurnProcess.vue:178; frontend-slint/src/timeline_text.rs:251.
Why: prefix.
In the model, one step leads into this bug: "按条目身份匹配思考".

0. Start: 相同前缀的新思考<br/>第一轮已完成<br/>活动第1轮，展示第1轮<br/>旧思考保持完整
1. 下一轮开始输出. Now: 相同前缀的新思考<br/>第二轮正在思考<br/>活动第2轮，展示第2轮<br/>旧思考保持完整
2. 按条目身份匹配思考. Now: 相同前缀的新思考<br/>迟到数据处理结束<br/>活动第2轮，展示第2轮<br/>旧思考被新文字覆盖

With the fix, step 2 (按条目身份匹配思考) takes you to: 相同前缀的新思考<br/>迟到数据处理结束<br/>活动第2轮，展示第2轮<br/>旧思考保持完整.

## 2. 迟到的持久快照<br/>迟到数据处理结束<br/>活动第2轮，展示第1轮<br/>旧思考保持完整

Rule broken: A newer durable revision with an older model identity cannot replace live text; mutator: frontend/src/realtime/chat/threadViewStore.ts:250.
Why: snapshot.
In the model, one step leads into this bug: "收到较旧模型条目的快照".

0. Start: 迟到的持久快照<br/>第一轮已完成<br/>活动第1轮，展示第1轮<br/>旧思考保持完整
1. 下一轮开始输出. Now: 迟到的持久快照<br/>第二轮正在思考<br/>活动第2轮，展示第2轮<br/>旧思考保持完整
2. 收到较旧模型条目的快照. Now: 迟到的持久快照<br/>迟到数据处理结束<br/>活动第2轮，展示第1轮<br/>旧思考保持完整

With the fix, step 2 (收到较旧模型条目的快照) takes you to: 迟到的持久快照<br/>迟到数据处理结束<br/>活动第2轮，展示第2轮<br/>旧思考保持完整.

## 3. 迟到的旧轮次增量<br/>迟到数据处理结束<br/>活动第2轮，展示第1轮<br/>旧思考保持完整

Rule broken: Old model tails cannot move or append to the current thought; mutator: frontend/src/realtime/chat/threadViewStore.ts:673.
Why: tail.
In the model, one step leads into this bug: "收到旧轮次增量".

0. Start: 迟到的旧轮次增量<br/>第一轮已完成<br/>活动第1轮，展示第1轮<br/>旧思考保持完整
1. 下一轮开始输出. Now: 迟到的旧轮次增量<br/>第二轮正在思考<br/>活动第2轮，展示第2轮<br/>旧思考保持完整
2. 收到旧轮次增量. Now: 迟到的旧轮次增量<br/>迟到数据处理结束<br/>活动第2轮，展示第1轮<br/>旧思考保持完整

With the fix, step 2 (收到旧轮次增量) takes you to: 迟到的旧轮次增量<br/>迟到数据处理结束<br/>活动第2轮，展示第2轮<br/>旧思考保持完整.
