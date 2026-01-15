<template>
  <div class="admin-view">
    <div class="admin-toolbar">
      <el-input v-model="keyword" placeholder="搜索用户名或邮箱" size="small" class="toolbar-input" />
      <el-button type="primary" size="small" @click="loadUsers">查询</el-button>
      <el-button type="primary" size="small" @click="openDialog">新增用户</el-button>
    </div>
    <div class="admin-table-wrapper">
      <el-table :data="adminStore.users" stripe height="100%">
        <el-table-column prop="username" label="用户名" />
        <el-table-column prop="email" label="邮箱" />
        <el-table-column prop="access_level" label="等级" width="120">
          <template #default="scope">
            <el-select v-model="scope.row.access_level" size="small" @change="updateLevel(scope.row)">
              <el-option label="A" value="A" />
              <el-option label="B" value="B" />
              <el-option label="C" value="C" />
            </el-select>
          </template>
        </el-table-column>
        <el-table-column prop="status" label="状态" width="120">
          <template #default="scope">
            <el-select v-model="scope.row.status" size="small" @change="updateStatus(scope.row)">
              <el-option label="active" value="active" />
              <el-option label="disabled" value="disabled" />
            </el-select>
          </template>
        </el-table-column>
        <el-table-column prop="roles" label="角色" />
        <el-table-column label="重置密码" width="240">
          <template #default="scope">
            <div class="admin-reset-password">
              <el-input
                v-model="resetPasswords[scope.row.id]"
                size="small"
                type="password"
                placeholder="新密码"
                show-password
              />
              <el-button
                size="small"
                type="primary"
                :loading="resetLoading[scope.row.id]"
                @click="handleResetPassword(scope.row)"
              >
                重置
              </el-button>
            </div>
          </template>
        </el-table-column>
        <el-table-column label="工具权限" width="120">
          <template #default="scope">
            <el-button size="small" @click="openToolDialog(scope.row)">配置</el-button>
          </template>
        </el-table-column>
      </el-table>
    </div>

    <el-dialog v-model="dialogVisible" title="新增用户" width="420px">
      <el-form :model="form" label-position="top">
        <el-form-item label="用户名">
          <el-input v-model="form.username" />
        </el-form-item>
        <el-form-item label="邮箱">
          <el-input v-model="form.email" />
        </el-form-item>
        <el-form-item label="密码">
          <el-input v-model="form.password" type="password" />
        </el-form-item>
        <el-form-item label="等级">
          <el-select v-model="form.access_level">
            <el-option label="A" value="A" />
            <el-option label="B" value="B" />
            <el-option label="C" value="C" />
          </el-select>
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="dialogVisible = false">取消</el-button>
        <el-button type="primary" @click="submit">提交</el-button>
      </template>
    </el-dialog>

    <el-dialog v-model="toolDialogVisible" title="工具权限配置" width="720px">
      <div v-loading="toolDialogLoading">
        <el-form :model="toolForm" label-position="top">
          <el-form-item label="用户">
            <span>{{ selectedUser?.username || '-' }}</span>
          </el-form-item>
          <el-form-item label="权限策略">
            <el-switch
              v-model="toolForm.use_default"
              active-text="使用默认策略"
              inactive-text="自定义工具白名单"
            />
            <el-text type="info" size="small">
              等级规则：A 默认全量工具；B 隐藏技能工具；C 隐藏技能与知识库工具。
            </el-text>
          </el-form-item>
          <el-form-item label="工具白名单">
            <el-select
              v-model="toolForm.allowed_tools"
              multiple
              filterable
              style="width: 100%"
              :disabled="toolForm.use_default"
            >
              <el-option-group v-for="group in toolGroups" :key="group.label" :label="group.label">
                <el-option
                  v-for="option in group.options"
                  :key="option.value"
                  :label="option.label"
                  :value="option.value"
                />
              </el-option-group>
            </el-select>
          </el-form-item>
        </el-form>
      </div>
      <template #footer>
        <el-button @click="toolDialogVisible = false">取消</el-button>
        <el-button type="primary" @click="saveToolAccess">保存</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup>
import { computed, onMounted, reactive, ref, watch } from 'vue';
import { ElMessage, ElMessageBox } from 'element-plus';

import { useAdminStore } from '@/stores/admin';

const adminStore = useAdminStore();
const keyword = ref('');
const dialogVisible = ref(false);
const toolDialogVisible = ref(false);
const toolDialogLoading = ref(false);
const selectedUser = ref(null);
// 重置密码输入与加载状态缓存
const resetPasswords = reactive({});
const resetLoading = reactive({});
const form = reactive({
  username: '',
  email: '',
  password: '',
  access_level: 'A'
});
// 工具权限配置表单（按用户）
const toolForm = reactive({
  agent_id: null,
  use_default: true,
  allowed_tools: []
});

const agents = computed(() => adminStore.agents || []);
const toolGroups = computed(() => {
  const catalog = adminStore.wunderTools || {};
  const buildOptions = (list) =>
    (list || []).map((item) => ({
      label: item.name,
      value: item.name,
      description: item.description || ''
    }));
  return [
    { label: '内置工具', options: buildOptions(catalog.builtin_tools) },
    { label: 'MCP 工具', options: buildOptions(catalog.mcp_tools) },
    { label: '技能工具', options: buildOptions(catalog.skills) },
    { label: '知识库工具', options: buildOptions(catalog.knowledge_tools) }
  ].filter((group) => group.options.length > 0);
});

const loadUsers = async () => {
  await adminStore.loadUsers(keyword.value ? { keyword: keyword.value } : {});
};

const openDialog = () => {
  dialogVisible.value = true;
};

const submit = async () => {
  try {
    await adminStore.createUser(form);
    dialogVisible.value = false;
    form.username = '';
    form.email = '';
    form.password = '';
    form.access_level = 'A';
    ElMessage.success('创建成功');
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '创建失败');
  }
};

const updateStatus = async (row) => {
  try {
    await adminStore.updateUser(row.id, { status: row.status });
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '更新失败');
  }
};

const updateLevel = async (row) => {
  try {
    await adminStore.updateUser(row.id, { access_level: row.access_level });
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '更新失败');
  }
};

const handleResetPassword = async (row) => {
  const nextPassword = String(resetPasswords[row.id] || '').trim();
  if (!nextPassword) {
    ElMessage.warning('请输入新密码');
    return;
  }
  try {
    // 重置前二次确认，避免误操作
    await ElMessageBox.confirm(
      `确认重置用户 ${row.username} 密码吗？`,
      '提示',
      {
        type: 'warning',
        confirmButtonText: '确认',
        cancelButtonText: '取消'
      }
    );
  } catch (error) {
    return;
  }
  resetLoading[row.id] = true;
  try {
    await adminStore.resetUserPassword(row.id, { password: nextPassword });
    resetPasswords[row.id] = '';
    ElMessage.success('密码已重置');
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '重置失败');
  } finally {
    resetLoading[row.id] = false;
  }
};

const resetToolForm = () => {
  toolForm.agent_id = null;
  toolForm.use_default = true;
  toolForm.allowed_tools = [];
};

const syncToolForm = () => {
  if (!selectedUser.value || !toolForm.agent_id) return;
  // 根据后端白名单回填当前用户的工具权限
  const accessList = adminStore.userToolAccess[selectedUser.value.id] || [];
  const matched = accessList.find((item) => item.agent_id === toolForm.agent_id);
  if (matched) {
    toolForm.use_default = false;
    toolForm.allowed_tools = Array.isArray(matched.allowed_tools) ? [...matched.allowed_tools] : [];
  } else {
    toolForm.use_default = true;
    toolForm.allowed_tools = [];
  }
};

const openToolDialog = async (user) => {
  selectedUser.value = user;
  toolDialogVisible.value = true;
  toolDialogLoading.value = true;
  resetToolForm();
  try {
    const results = await Promise.all([
      adminStore.loadAgents(),
      adminStore.loadWunderTools(),
      adminStore.loadUserToolAccess(user.id)
    ]);
    const accessList = results[2] || [];
    if (!toolForm.agent_id) {
      toolForm.agent_id = accessList[0]?.agent_id || agents.value[0]?.id || null;
    }
    syncToolForm();
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '加载失败');
  } finally {
    toolDialogLoading.value = false;
  }
};

const saveToolAccess = async () => {
  if (!selectedUser.value) return;
  if (!toolForm.agent_id) {
    ElMessage.warning('暂无可用智能体，请先创建');
    return;
  }
  try {
    await adminStore.updateUserToolAccess(selectedUser.value.id, {
      agent_id: toolForm.agent_id,
      allowed_tools: toolForm.use_default ? null : toolForm.allowed_tools
    });
    ElMessage.success('保存成功');
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '保存失败');
  }
};

watch(
  () => toolForm.agent_id,
  () => {
    if (toolDialogVisible.value) {
      syncToolForm();
    }
  }
);

watch(
  () => toolForm.use_default,
  (value) => {
    if (value) {
      toolForm.allowed_tools = [];
    }
  }
);

onMounted(loadUsers);
</script>
