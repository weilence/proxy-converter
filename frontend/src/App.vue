<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { api } from './api/client'
import AdminView from './views/AdminView.vue'
import LoginView from './views/LoginView.vue'

// null while probing the session on startup.
const loggedIn = ref<boolean | null>(null)

onMounted(async () => {
  loggedIn.value = await api.loggedIn()
})
</script>

<template>
  <UApp>
    <AdminView v-if="loggedIn === true" @logout="loggedIn = false" />
    <LoginView v-else-if="loggedIn === false" @logged-in="loggedIn = true" />
  </UApp>
</template>
