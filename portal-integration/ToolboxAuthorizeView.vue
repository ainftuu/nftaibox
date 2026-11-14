<script setup lang="ts">
import { computed, ref, onMounted, onBeforeUnmount } from 'vue'
import { useRoute } from 'vue-router'
import { apiClient } from '@/api/client'
import AuthLayout from '@/components/layout/AuthLayout.vue'
const route = useRoute()
const busy = ref(false)
const error = ref('')
const port = computed(() => Number(route.query.port))
const state = computed(() => String(route.query.state || ''))
// HTTPS -> loopback HTTP form navigation otherwise sends Origin: null under
// strict-origin-when-cross-origin (Fetch Standard, append request Origin).
// Send only the site's origin, never the authorization URL or token values.
const referrerMeta = document.createElement('meta')
referrerMeta.name = 'referrer'
referrerMeta.content = 'origin'
onMounted(() => document.head.append(referrerMeta))
onBeforeUnmount(() => referrerMeta.remove())
const valid = computed(() => /^\d{4,5}$/.test(String(route.query.port)) && port.value >= 1024 && port.value <= 65535 && /^[0-9a-f-]{36}$/.test(state.value))
async function authorize() {
  if (!valid.value || busy.value) return
  busy.value = true
  error.value = ''
  try {
    // Issue a separate desktop session. Never share/rotate the browser refresh token.
    const { data } = await apiClient.post('/auth/toolbox-session')
    if (!data.access_token || !data.refresh_token) throw new Error('授权接口未返回完整会话，请联系站长检查部署。')
    const form = document.createElement('form')
    form.method = 'POST'
    // Never accept a user-supplied callback hostname or arbitrary URL.
    form.action = `http://127.0.0.1:${port.value}/authorize`
    for (const [name, value] of Object.entries({ state: state.value, access_token: data.access_token, refresh_token: data.refresh_token })) {
      const input = document.createElement('input')
      input.type = 'hidden'; input.name = name; input.value = String(value)
      form.append(input)
    }
    document.body.append(form)
    form.submit()
  } catch (e) {
    error.value = e && typeof e === 'object' && 'message' in e ? String(e.message) : '授权失败，请重试。'
    busy.value = false
  }
}
</script>
<template>
  <AuthLayout>
    <div class="toolbox-card">
      <p class="toolbox-kicker">NFTAI 工具箱</p>
      <h1>让 AI 编程准备就绪</h1>
      <p>授权本机工具箱读取你的账号信息、余额和 API 密钥，帮你安装并配置 AI 工具。</p>
      <p class="toolbox-note">仅当你刚刚在本机 NFTAI 工具箱点击登录时，才继续授权。密码不会发送给工具箱。</p>
      <button :disabled="!valid || busy" @click="authorize">{{ busy ? '正在返回工具箱…' : '授权并返回工具箱' }}</button>
      <p v-if="!valid" role="alert">授权链接无效，请从 NFTAI 工具箱重新发起登录。</p>
      <p v-if="error" role="alert">{{ error }}</p>
    </div>
  </AuthLayout>
</template>
<style scoped>
.toolbox-card{color:#121417}.toolbox-kicker{color:#5a3bff!important;font-size:12px!important;margin:0 0 12px}.toolbox-card h1{font-size:26px;font-weight:700;margin:12px 0 24px;letter-spacing:-.03em}.toolbox-card p{font-size:13px;line-height:1.9;color:#686b73}.toolbox-note{background:#f4f5f1;padding:16px;border:1px solid #e3e5df;border-radius:12px;margin:20px 0}.toolbox-card button{width:100%;padding:14px;background:#121417;color:white;border:0;border-radius:999px;font-size:14px;font-weight:600;cursor:pointer}.toolbox-card button:hover:enabled{background:#262a31}.toolbox-card button:focus-visible{outline:2px solid #5a3bff;outline-offset:3px}.toolbox-card button:disabled{opacity:.5}.toolbox-card [role=alert]{color:#ba4437}
</style>
