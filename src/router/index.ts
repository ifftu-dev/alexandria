import { createRouter, createWebHistory } from 'vue-router'

const router = createRouter({
  history: createWebHistory(),
  routes: [
    // Onboarding (no wallet yet)
    {
      path: '/onboarding',
      name: 'onboarding',
      component: () => import('@/pages/Onboarding.vue'),
      meta: { layout: 'blank' },
    },

    // App routes (wallet required)
    {
      path: '/',
      redirect: '/home',
    },
    {
      path: '/home',
      name: 'home',
      component: () => import('@/pages/Home.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/courses',
      name: 'courses',
      component: () => import('@/pages/courses/Index.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/courses/:id',
      name: 'course-detail',
      component: () => import('@/pages/courses/Detail.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/dashboard/courses',
      name: 'dashboard-courses',
      component: () => import('@/pages/dashboard/Courses.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/dashboard/settings',
      name: 'dashboard-settings',
      component: () => import('@/pages/dashboard/Settings.vue'),
      meta: { layout: 'app' },
    },
  ],
})

export default router
