## 1. 帧栈 普通消息 → 输入法消息；刚发生过输入法写，位置：普通消息帧内

Rule broken: 会回到输入法的写调用只发生在消息泵轮的延迟闭包里，不占用任何系统消息帧。
In the model, one step leads into this bug: "（修复前）帧内直接写输入法".

0. Start: 帧栈空，消息泵轮
1. 系统送来普通消息. Now: 帧栈 普通消息
2. （修复前）帧内直接写输入法. Now: 帧栈 普通消息 → 输入法消息；刚发生过输入法写，位置：普通消息帧内

With the fix, step 2 (（修复前）帧内直接写输入法) cannot happen, so you stay at: 帧栈 普通消息.

## 2. 帧栈 普通消息；刚发生过输入法写，位置：普通消息帧内

Rule broken: 会回到输入法的写调用只发生在消息泵轮的延迟闭包里，不占用任何系统消息帧。
In the model, one step leads into this bug: "（修复前）帧内直接写输入法".

0. Start: 帧栈空，消息泵轮
1. 系统送来普通消息. Now: 帧栈 普通消息
2. （修复前）帧内直接写输入法. Now: 帧栈 普通消息；刚发生过输入法写，位置：普通消息帧内

With the fix, step 2 (（修复前）帧内直接写输入法) cannot happen, so you stay at: 帧栈 普通消息.

## 3. 帧栈 输入法消息 → 输入法消息；刚发生过输入法写，位置：输入法消息帧内

Rule broken: 会回到输入法的写调用只发生在消息泵轮的延迟闭包里，不占用任何系统消息帧。
In the model, one step leads into this bug: "（修复前）帧内直接写输入法".

0. Start: 帧栈空，消息泵轮
1. 输入法送来组合消息. Now: 帧栈 输入法消息
2. （修复前）帧内直接写输入法. Now: 帧栈 输入法消息 → 输入法消息；刚发生过输入法写，位置：输入法消息帧内

With the fix, step 2 (（修复前）帧内直接写输入法) cannot happen, so you stay at: 帧栈 输入法消息.

## 4. 帧栈 输入法消息；刚发生过输入法写，位置：输入法消息帧内

Rule broken: 会回到输入法的写调用只发生在消息泵轮的延迟闭包里，不占用任何系统消息帧。
In the model, one step leads into this bug: "（修复前）帧内直接写输入法".

0. Start: 帧栈空，消息泵轮
1. 输入法送来组合消息. Now: 帧栈 输入法消息
2. （修复前）帧内直接写输入法. Now: 帧栈 输入法消息；刚发生过输入法写，位置：输入法消息帧内

With the fix, step 2 (（修复前）帧内直接写输入法) cannot happen, so you stay at: 帧栈 输入法消息.

## 5. 帧栈 输入法消息 → 输入法消息；刚发生过输入法写，位置：输入法消息帧内

Rule broken: 输入法消息帧存活期间，本线程绝不再发起会回到输入法状态机的写调用。
In the model, one step leads into this bug: "（修复前）帧内直接写输入法".

0. Start: 帧栈空，消息泵轮
1. 输入法送来组合消息. Now: 帧栈 输入法消息
2. （修复前）帧内直接写输入法. Now: 帧栈 输入法消息 → 输入法消息；刚发生过输入法写，位置：输入法消息帧内

With the fix, step 2 (（修复前）帧内直接写输入法) cannot happen, so you stay at: 帧栈 输入法消息.

## 6. 帧栈 输入法消息；刚发生过输入法写，位置：输入法消息帧内

Rule broken: 输入法消息帧存活期间，本线程绝不再发起会回到输入法状态机的写调用。
In the model, one step leads into this bug: "（修复前）帧内直接写输入法".

0. Start: 帧栈空，消息泵轮
1. 输入法送来组合消息. Now: 帧栈 输入法消息
2. （修复前）帧内直接写输入法. Now: 帧栈 输入法消息；刚发生过输入法写，位置：输入法消息帧内

With the fix, step 2 (（修复前）帧内直接写输入法) cannot happen, so you stay at: 帧栈 输入法消息.

## 7. 帧栈 普通消息 → 窗口过程已卡死

Rule broken: 线程不会进入持锁重入的卡死吸收态。
In the model, one step leads into this bug: "（修复前）持锁调用捕获接口，重入再取锁". The steps below are the shortest path that TLC found.

0. Start: 帧栈空，消息泵轮
1. 系统送来普通消息. Now: 帧栈 普通消息
2. （修复前）持锁调用捕获接口，重入再取锁. Now: 帧栈 普通消息 → 窗口过程已卡死

With the fix, step 2 (（修复前）持锁调用捕获接口，重入再取锁) cannot happen, so you stay at: 帧栈 普通消息.

## 8. 帧栈 普通消息 → 输入法消息 → 输入法消息；刚发生过输入法写，位置：输入法消息帧内

Rule broken: 帧栈深度有界：同步重入链条不得超过“一条消息 + 一次输入法回弹”。
In the model, one step leads into this bug: "（修复前）帧内直接写输入法".

0. Start: 帧栈空，消息泵轮
1. 系统送来普通消息. Now: 帧栈 普通消息
2. （修复前）帧内直接写输入法. Now: 帧栈 普通消息 → 输入法消息；刚发生过输入法写，位置：普通消息帧内
3. （修复前）帧内直接写输入法. Now: 帧栈 普通消息 → 输入法消息 → 输入法消息；刚发生过输入法写，位置：输入法消息帧内

With the fix, step 2 (（修复前）帧内直接写输入法) cannot happen, so you stay at: 帧栈 普通消息.

## 9. 帧栈 输入法消息 → 输入法消息 → 输入法消息；刚发生过输入法写，位置：输入法消息帧内

Rule broken: 帧栈深度有界：同步重入链条不得超过“一条消息 + 一次输入法回弹”。
In the model, one step leads into this bug: "（修复前）帧内直接写输入法".

0. Start: 帧栈空，消息泵轮
1. 输入法送来组合消息. Now: 帧栈 输入法消息
2. （修复前）帧内直接写输入法. Now: 帧栈 输入法消息 → 输入法消息；刚发生过输入法写，位置：输入法消息帧内
3. （修复前）帧内直接写输入法. Now: 帧栈 输入法消息 → 输入法消息 → 输入法消息；刚发生过输入法写，位置：输入法消息帧内

With the fix, step 2 (（修复前）帧内直接写输入法) cannot happen, so you stay at: 帧栈 输入法消息.

## 10. 帧栈 普通消息 → 输入法消息 → 输入法消息；刚发生过输入法写，位置：输入法消息帧内

Rule broken: 输入法消息帧存活期间，本线程绝不再发起会回到输入法状态机的写调用。
In the model, one step leads into this bug: "（修复前）帧内直接写输入法".

0. Start: 帧栈空，消息泵轮
1. 系统送来普通消息. Now: 帧栈 普通消息
2. （修复前）帧内直接写输入法. Now: 帧栈 普通消息 → 输入法消息；刚发生过输入法写，位置：普通消息帧内
3. （修复前）帧内直接写输入法. Now: 帧栈 普通消息 → 输入法消息 → 输入法消息；刚发生过输入法写，位置：输入法消息帧内

With the fix, step 2 (（修复前）帧内直接写输入法) cannot happen, so you stay at: 帧栈 普通消息.

## 11. 帧栈 普通消息 → 输入法消息；刚发生过输入法写，位置：输入法消息帧内

Rule broken: 输入法消息帧存活期间，本线程绝不再发起会回到输入法状态机的写调用。
In the model, one step leads into this bug: "（修复前）帧内直接写输入法".

0. Start: 帧栈空，消息泵轮
1. 系统送来普通消息. Now: 帧栈 普通消息
2. （修复前）帧内直接写输入法. Now: 帧栈 普通消息 → 输入法消息；刚发生过输入法写，位置：普通消息帧内
3. （修复前）帧内直接写输入法. Now: 帧栈 普通消息 → 输入法消息；刚发生过输入法写，位置：输入法消息帧内

With the fix, step 2 (（修复前）帧内直接写输入法) cannot happen, so you stay at: 帧栈 普通消息.
