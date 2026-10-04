import { createRouter, createWebHistory } from 'vue-router'

const router = createRouter({
  history: createWebHistory(import.meta.env.BASE_URL),
  routes: [
    { path: '/', name: 'projects', component: () => import('@/views/ProjectsView.vue') },
    { path: '/workbench', name: 'workbench', component: () => import('@/views/WorkbenchView.vue') },
    { path: '/providers', name: 'providers', component: () => import('@/views/ProvidersView.vue') },
    { path: '/:pathMatch(.*)*', redirect: '/' },
  ],
})

export default router
