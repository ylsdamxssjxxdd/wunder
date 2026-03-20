<template>
  <section class="messenger-help-manual-panel">
    <iframe
      class="messenger-help-manual-frame"
      src="/docs/"
      title="wunder docs"
      referrerpolicy="no-referrer"
    ></iframe>
  </section>
</template>

<style scoped>
.messenger-help-manual-panel {
  display: flex;
  flex: 1;
  min-height: 0;
  height: 100%;
}

.messenger-help-manual-frame {
  border: 0;
  width: 100%;
  height: 100%;
  display: block;
  background: #ffffff;
}
</style>
