## 1. 过程展开，显示最新一页，还有更早的步骤

Rule broken: With older steps left and the newest page on screen, the reader must still be able to reach them. The reader's sentence: 还有更早的过程时，向上滑或点击都要能真的看到它们.
Why: 视图重建时把续读位置弄丢了，界面还说有更早的步骤，点下去读到的还是同一页.
In the model, 2 different steps lead into this bug: "点「查看更早的过程」" and "视图被重建" (the state stays the same). The steps below are the shortest path that TLC found.

0. Start: 过程收着，还没读内容，还有更早的步骤
1. 展开工作过程. Now: 过程展开，还没读内容，还有更早的步骤
2. 点「查看更早的过程」. Now: 过程展开，显示最新一页，还有更早的步骤

With the fix, step 2 (点「查看更早的过程」) cannot happen, so you stay at: 过程展开，还没读内容，还有更早的步骤.

## 2. 过程展开，显示最新一页，还有更早的步骤

Rule broken: A loaded page keeps the cursor that continues it, whatever the view does.
Why: 视图重建时把续读位置弄丢了，界面还说有更早的步骤，点下去读到的还是同一页.
In the model, 2 different steps lead into this bug: "点「查看更早的过程」" and "视图被重建" (the state stays the same). The steps below are the shortest path that TLC found.

0. Start: 过程收着，还没读内容，还有更早的步骤
1. 展开工作过程. Now: 过程展开，还没读内容，还有更早的步骤
2. 点「查看更早的过程」. Now: 过程展开，显示最新一页，还有更早的步骤

With the fix, step 2 (点「查看更早的过程」) cannot happen, so you stay at: 过程展开，还没读内容，还有更早的步骤.
