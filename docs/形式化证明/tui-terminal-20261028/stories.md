## 1. 第 1 轮：原始错误之后又出现空输出错误

Rule broken: A reported request error must not be followed by a synthetic empty-output error.
In the model, one step leads into this bug: "流关闭并结算".

0. Start: 第 1 轮：新请求未继承旧错误
1. 收到并显示原始错误. Now: 第 1 轮：已显示原始错误
2. 收到后续取消终态. Now: 第 1 轮：终态已到达
3. 流关闭并结算. Now: 第 1 轮：原始错误之后又出现空输出错误

With the fix, step 2 (收到后续取消终态) takes you to: 第 1 轮：取消终态保留原始错误.

## 2. 第 2 轮：原始错误之后又出现空输出错误

Rule broken: A reported request error must not be followed by a synthetic empty-output error.
In the model, one step leads into this bug: "流关闭并结算".

0. Start: 第 1 轮：新请求未继承旧错误
1. 无输出直接收到终态. Now: 第 1 轮：终态已到达
2. 流关闭并结算. Now: 第 1 轮：无输出时正常提示错误
3. 建立下一轮流并重置状态. Now: 第 2 轮：新请求未继承旧错误
4. 收到并显示原始错误. Now: 第 2 轮：已显示原始错误
5. 收到后续取消终态. Now: 第 2 轮：终态已到达
6. 流关闭并结算. Now: 第 2 轮：原始错误之后又出现空输出错误

With the fix, step 5 (收到后续取消终态) takes you to: 第 2 轮：取消终态保留原始错误.
