import { createApp } from 'vue'
import { createPinia } from 'pinia'

import App from './App.vue'
import router from './router'

import 'vue-sonner/style.css'

import './assets/theme.css'
import './assets/app.css'
import './assets/sonner.css'

const app = createApp(App)

app.use(createPinia())
app.use(router)

app.mount('#app')
