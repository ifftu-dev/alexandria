import { createRouter, createWebHistory } from 'vue-router'

const router = createRouter({
  history: createWebHistory(),
  routes: [
    // ---- Auth (blank layout) ----
    {
      path: '/onboarding',
      name: 'onboarding',
      component: () => import('@/pages/Onboarding.vue'),
      meta: { layout: 'blank' },
    },
    {
      path: '/unlock',
      name: 'unlock',
      component: () => import('@/pages/Unlock.vue'),
      meta: { layout: 'blank' },
    },

    // ---- App routes (app layout, wallet required) ----
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

    // Courses — catalog
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

    // Instructor — course authoring
    {
      path: '/instructor/courses/new',
      name: 'course-create',
      component: () => import('@/pages/instructor/CourseNew.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/instructor/courses/:id',
      name: 'course-edit',
      component: () => import('@/pages/instructor/CourseEdit.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/instructor/tutorials/new',
      name: 'tutorial-create',
      component: () => import('@/pages/instructor/TutorialNew.vue'),
      meta: { layout: 'app' },
    },

    // Learning player
    {
      path: '/learn/:id',
      name: 'learn',
      component: () => import('@/pages/learn/Player.vue'),
      meta: { layout: 'app' },
    },

    // Classrooms
    {
      path: '/classrooms',
      name: 'classrooms',
      component: () => import('@/pages/classrooms/Index.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/classrooms/:id',
      name: 'classroom',
      component: () => import('@/pages/classrooms/Classroom.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/classrooms/:id/settings',
      name: 'classroom-settings',
      component: () => import('@/pages/classrooms/Settings.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/classrooms/:id/requests',
      name: 'classroom-requests',
      component: () => import('@/pages/classrooms/JoinRequests.vue'),
      meta: { layout: 'app' },
    },

    // Live Tutoring
    {
      path: '/tutoring',
      name: 'tutoring',
      component: () => import('@/pages/tutoring/Index.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/tutoring/:id',
      name: 'tutoring-session',
      component: () => import('@/pages/tutoring/Session.vue'),
      meta: { layout: 'app' },
    },

    // Skills & Taxonomy
    {
      path: '/skills',
      name: 'skills',
      component: () => import('@/pages/skills/Index.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/skills/:id',
      name: 'skill-detail',
      component: () => import('@/pages/skills/Detail.vue'),
      meta: { layout: 'app' },
    },

    // Governance
    {
      path: '/governance',
      name: 'governance',
      component: () => import('@/pages/governance/Index.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/governance/:id',
      name: 'dao-detail',
      component: () => import('@/pages/governance/DaoDetail.vue'),
      meta: { layout: 'app' },
    },

    // Dashboard
    {
      path: '/dashboard/courses',
      name: 'dashboard-courses',
      component: () => import('@/pages/dashboard/Courses.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/dashboard/credentials',
      name: 'dashboard-credentials',
      component: () => import('@/pages/dashboard/Credentials.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/dashboard/reputation',
      name: 'dashboard-reputation',
      component: () => import('@/pages/dashboard/Reputation.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/dashboard/network',
      name: 'dashboard-network',
      component: () => import('@/pages/dashboard/Network.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/dashboard/sync',
      name: 'dashboard-sync',
      component: () => import('@/pages/dashboard/Sync.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/dashboard/sentinel',
      name: 'dashboard-sentinel',
      component: () => import('@/pages/dashboard/Sentinel.vue'),
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
