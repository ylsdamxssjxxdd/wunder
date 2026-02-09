<template>
  <div class="admin-view">
    <div class="admin-toolbar">
      <el-input
        v-model="keyword"
        :placeholder="t('admin.users.search')"
        size="small"
        class="toolbar-input"
      />
      <el-button type="primary" size="small" @click="loadUsers">{{ t('admin.users.query') }}</el-button>
      <el-button type="primary" size="small" @click="openDialog">{{ t('admin.users.create') }}</el-button>
    </div>
    <div class="admin-table-wrapper">
      <el-table :data="adminStore.users" stripe height="100%">
        <el-table-column prop="username" :label="t('admin.users.username')" />
        <el-table-column prop="email" :label="t('admin.users.email')" />
        <el-table-column prop="access_level" :label="t('admin.users.level')" width="120">
          <template #default="scope">
            <el-select v-model="scope.row.access_level" size="small" @change="updateLevel(scope.row)">
              <el-option label="A" value="A" />
              <el-option label="B" value="B" />
              <el-option label="C" value="C" />
            </el-select>
          </template>
        </el-table-column>
        <el-table-column prop="status" :label="t('common.status')" width="120">
          <template #default="scope">
            <el-select v-model="scope.row.status" size="small" @change="updateStatus(scope.row)">
              <el-option label="active" value="active" />
              <el-option label="disabled" value="disabled" />
            </el-select>
          </template>
        </el-table-column>
        <el-table-column prop="roles" :label="t('admin.users.roles')" />
        <el-table-column :label="t('admin.users.resetPassword')" width="240">
          <template #default="scope">
            <div class="admin-reset-password">
              <el-input
                v-model="resetPasswords[scope.row.id]"
                size="small"
                type="password"
                :placeholder="t('admin.users.resetPassword.placeholder')"
                show-password
              />
              <el-button
                size="small"
                type="primary"
                :loading="resetLoading[scope.row.id]"
                @click="handleResetPassword(scope.row)"
              >
                {{ t('common.reset') }}
              </el-button>
            </div>
          </template>
        </el-table-column>
        <el-table-column :label="t('admin.users.tools')" width="120">
          <template #default="scope">
            <el-button size="small" @click="openToolDialog(scope.row)">
              {{ t('admin.users.tools.configure') }}
            </el-button>
          </template>
        </el-table-column>
      </el-table>
    </div>

    <el-dialog v-model="dialogVisible" :title="t('admin.users.create')" width="420px">
      <el-form :model="form" label-position="top">
        <el-form-item :label="t('admin.users.username')">
          <el-input v-model="form.username" />
        </el-form-item>
        <el-form-item :label="t('admin.users.email')">
          <el-input v-model="form.email" />
        </el-form-item>
        <el-form-item :label="t('admin.users.password')">
          <el-input v-model="form.password" type="password" />
        </el-form-item>
        <el-form-item :label="t('admin.users.level')">
          <el-select v-model="form.access_level">
            <el-option label="A" value="A" />
            <el-option label="B" value="B" />
            <el-option label="C" value="C" />
          </el-select>
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="dialogVisible = false">{{ t('common.cancel') }}</el-button>
        <el-button type="primary" @click="submit">{{ t('common.submit') }}</el-button>
      </template>
    </el-dialog>

    <el-dialog v-model="toolDialogVisible" :title="t('admin.users.tools.dialog.title')" width="720px">
      <div v-loading="toolDialogLoading">
        <el-form :model="toolForm" label-position="top">
          <el-form-item :label="t('admin.users.tools.user')">
            <span>{{ selectedUser?.username || '-' }}</span>
          </el-form-item>
          <el-form-item :label="t('admin.users.tools.policy')">
            <el-switch
              v-model="toolForm.use_default"
              :active-text="t('admin.users.tools.policy.default')"
              :inactive-text="t('admin.users.tools.policy.custom')"
            />
            <el-text type="info" size="small">
              {{ t('admin.users.tools.policy.tip') }}
            </el-text>
          </el-form-item>
          <el-form-item :label="t('admin.users.tools.whitelist')">
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
        <el-button @click="toolDialogVisible = false">{{ t('common.cancel') }}</el-button>
        <el-button type="primary" @click="saveToolAccess">{{ t('common.save') }}</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue';
import { ElMessage, ElMessageBox } from 'element-plus';

import { useI18n } from '@/i18n';
import { useAdminStore } from '@/stores/admin';
import { showApiError } from '@/utils/apiError';

const adminStore = useAdminStore();
const { t } = useI18n();
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
    { label: t('admin.agents.tools.builtin'), options: buildOptions(catalog.builtin_tools) },
    { label: t('admin.agents.tools.mcp'), options: buildOptions(catalog.mcp_tools) },
    { label: t('admin.agents.tools.skills'), options: buildOptions(catalog.skills) },
    { label: t('admin.agents.tools.knowledge'), options: buildOptions(catalog.knowledge_tools) }
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
    ElMessage.success(t('admin.users.createSuccess'));
  } catch (error) {
    showApiError(error, t('admin.users.createFailed'));
  }
};

const updateStatus = async (row) => {
  try {
    await adminStore.updateUser(row.id, { status: row.status });
  } catch (error) {
    showApiError(error, t('admin.users.updateFailed'));
  }
};

const updateLevel = async (row) => {
  try {
    await adminStore.updateUser(row.id, { access_level: row.access_level });
  } catch (error) {
    showApiError(error, t('admin.users.updateFailed'));
  }
};

const handleResetPassword = async (row) => {
  const nextPassword = String(resetPasswords[row.id] || '').trim();
  if (!nextPassword) {
    ElMessage.warning(t('admin.users.resetPassword.required'));
    return;
  }
  try {
    // 重置前二次确认，避免误操作
    await ElMessageBox.confirm(
      t('admin.users.resetPassword.confirm', { name: row.username }),
      t('common.notice'),
      {
        type: 'warning',
        confirmButtonText: t('common.confirm'),
        cancelButtonText: t('common.cancel')
      }
    );
  } catch (error) {
    return;
  }
  resetLoading[row.id] = true;
  try {
    await adminStore.resetUserPassword(row.id, { password: nextPassword });
    resetPasswords[row.id] = '';
    ElMessage.success(t('admin.users.resetPassword.success'));
  } catch (error) {
    showApiError(error, t('admin.users.resetPassword.failed'));
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
    showApiError(error, t('admin.users.loadFailed'));
  } finally {
    toolDialogLoading.value = false;
  }
};

const saveToolAccess = async () => {
  if (!selectedUser.value) return;
  if (!toolForm.agent_id) {
    ElMessage.warning(t('admin.users.tools.noAgent'));
    return;
  }
  try {
    await adminStore.updateUserToolAccess(selectedUser.value.id, {
      agent_id: toolForm.agent_id,
      allowed_tools: toolForm.use_default ? null : toolForm.allowed_tools
    });
    ElMessage.success(t('admin.users.saveSuccess'));
  } catch (error) {
    showApiError(error, t('admin.users.saveFailed'));
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