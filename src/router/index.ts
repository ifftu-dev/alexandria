import { createRouter, createWebHistory } from 'vue-router'

const router = createRouter({
  history: createWebHistory(),
  routes: [
    // ---- Auth (blank layout) ----
    {
      path: '/profiles',
      name: 'profiles',
      component: () => import('@/pages/ProfileSelect.vue'),
      meta: { layout: 'blank' },
    },
    {
      path: '/onboarding',
      name: 'onboarding',
      component: () => import('@/pages/Onboarding.vue'),
      meta: { layout: 'blank' },
    },
    {
      // Legacy /unlock — redirects to picker now. Kept so any cached
      // deep link from the pre-multi-user release still routes somewhere
      // sane.
      path: '/unlock',
      redirect: '/profiles',
    },
    {
      // Holding screen for minor learners whose profile awaits guardian
      // activation. The global guard below funnels gated profiles here.
      path: '/guardian-gate',
      name: 'guardian-gate',
      component: () => import('@/pages/GuardianGate.vue'),
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

    // Guardian (parent role) — children oversight
    {
      path: '/guardian',
      name: 'guardian-children',
      component: () => import('@/pages/guardian/Children.vue'),
      meta: { layout: 'app', requiresRole: 'parent' },
    },
    {
      path: '/guardian/child/:linkId',
      name: 'guardian-child-detail',
      component: () => import('@/pages/guardian/ChildDetail.vue'),
      meta: { layout: 'app', requiresRole: 'parent' },
    },

    // Instructor — dashboard, learners, inbox
    {
      path: '/instructor',
      name: 'instructor-dashboard',
      component: () => import('@/pages/instructor/Dashboard.vue'),
      meta: { layout: 'app', requiresInstructorMode: true },
    },
    {
      path: '/instructor/courses/:id/learners',
      name: 'instructor-course-learners',
      component: () => import('@/pages/instructor/CourseLearners.vue'),
      meta: { layout: 'app', requiresInstructorMode: true },
    },
    {
      path: '/instructor/inbox',
      name: 'instructor-inbox',
      component: () => import('@/pages/instructor/Inbox.vue'),
      meta: { layout: 'app', requiresInstructorMode: true },
    },
    {
      path: '/instructor/review/:id',
      name: 'instructor-review',
      component: () => import('@/pages/instructor/SubmissionReview.vue'),
      meta: { layout: 'app', requiresInstructorMode: true },
    },

    // Instructor — unified composer (courses + tutorials)
    {
      path: '/instructor/composer/new',
      name: 'composer-new',
      component: () => import('@/pages/instructor/Composer.vue'),
      meta: { layout: 'app', requiresInstructorMode: true },
    },
    {
      path: '/instructor/composer/:id',
      name: 'composer',
      component: () => import('@/pages/instructor/Composer.vue'),
      meta: { layout: 'app', requiresInstructorMode: true },
    },
    {
      path: '/instructor/courses',
      name: 'instructor-courses',
      component: () => import('@/pages/instructor/MyCourses.vue'),
      meta: { layout: 'app', requiresInstructorMode: true },
    },
    // Legacy authoring routes — the composer supersedes them.
    { path: '/instructor/courses/new', redirect: '/instructor/composer/new?kind=course' },
    { path: '/instructor/courses/:id', redirect: (to) => `/instructor/composer/${to.params.id}` },
    { path: '/instructor/tutorials/new', redirect: '/instructor/composer/new?kind=tutorial' },

    // Opinions (Field Commentary — credentialed video takes)
    {
      path: '/opinions',
      name: 'opinions',
      component: () => import('@/pages/opinions/Index.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/opinions/new',
      name: 'opinion-create',
      component: () => import('@/pages/opinions/New.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/opinions/:id',
      name: 'opinion-detail',
      component: () => import('@/pages/opinions/Detail.vue'),
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
      path: '/skills/import',
      name: 'skills-import',
      component: () => import('@/pages/skills/BootstrapUpload.vue'),
      meta: { layout: 'app' },
    },
    { path: '/skills/bootstrap', redirect: '/skills/import' },
    {
      path: '/assessment/:skillId',
      name: 'assessment',
      component: () => import('@/pages/learn/AssessmentRunner.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/skills/:id',
      name: 'skill-detail',
      component: () => import('@/pages/skills/Detail.vue'),
      meta: { layout: 'app' },
    },

    // Learning goals + instructor public graphs
    {
      path: '/goals',
      name: 'goals',
      component: () => import('@/pages/goals/Index.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/u/:id',
      name: 'user-profile',
      component: () => import('@/pages/u/InstructorGraph.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/profile',
      name: 'my-profile',
      component: () => import('@/pages/ProfileMe.vue'),
      meta: { layout: 'app' },
    },

    // Community (formerly "Governance")
    {
      path: '/community',
      name: 'community',
      component: () => import('@/pages/governance/Index.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/community/:id',
      name: 'community-detail',
      component: () => import('@/pages/governance/DaoDetail.vue'),
      meta: { layout: 'app' },
    },
    // Legacy /governance* paths — redirect so old deeplinks/bookmarks resolve.
    { path: '/governance', redirect: '/community' },
    { path: '/governance/:id', redirect: (to) => `/community/${to.params.id}` },

    // Dashboard surfaces — flattened to plain top-level paths.
    {
      path: '/learning',
      name: 'learning',
      component: () => import('@/pages/dashboard/Courses.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/credentials',
      name: 'credentials',
      component: () => import('@/pages/dashboard/Credentials.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/sponsor',
      name: 'sponsor',
      component: () => import('@/pages/dashboard/Sponsor.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/credentials/:id',
      name: 'credential-detail',
      component: () => import('@/pages/dashboard/CredentialDetail.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/reputation',
      name: 'reputation',
      component: () => import('@/pages/dashboard/Reputation.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/network',
      name: 'network-status',
      component: () => import('@/pages/dashboard/Network.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/sync',
      name: 'sync-status',
      component: () => import('@/pages/dashboard/Sync.vue'),
      meta: { layout: 'app' },
    },
    // Legacy /dashboard/* paths — redirect so old deeplinks/bookmarks resolve.
    { path: '/dashboard/courses', redirect: '/learning' },
    { path: '/dashboard/credentials', redirect: '/credentials' },
    { path: '/dashboard/credentials/:id', redirect: (to) => `/credentials/${to.params.id}` },
    { path: '/dashboard/sponsor', redirect: '/sponsor' },
    { path: '/dashboard/reputation', redirect: '/reputation' },
    { path: '/dashboard/network', redirect: '/network' },
    { path: '/dashboard/sync', redirect: '/sync' },
    {
      path: '/dashboard/sentinel',
      name: 'dashboard-sentinel',
      component: () => import('@/pages/dashboard/Sentinel.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/dashboard/sentinel/propose-prior',
      name: 'dashboard-sentinel-propose-prior',
      component: () => import('@/pages/dashboard/sentinel/ProposePrior.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/dashboard/sentinel/holdout-evaluate',
      name: 'dashboard-sentinel-holdout-evaluate',
      component: () => import('@/pages/dashboard/sentinel/HoldoutEvaluate.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/dashboard/sentinel/cheat-test',
      name: 'dashboard-sentinel-cheat-test',
      component: () => import('@/pages/dashboard/sentinel/CheatTest.vue'),
      meta: { layout: 'app' },
    },
    // Community plugins.
    {
      path: '/plugins',
      name: 'plugins-installed',
      component: () => import('@/pages/plugins/Installed.vue'),
      meta: { layout: 'app' },
    },
    {
      path: '/plugins/browse',
      name: 'plugins-browse',
      component: () => import('@/pages/plugins/Browse.vue'),
      meta: { layout: 'app' },
    },

    // Plugin documentation — full page (reached from Settings → Plugins).
    // Declared before the catch-all settings route so it isn't swallowed
    // by `/settings/:section?`.
    {
      path: '/settings/plugins/:cid/docs',
      name: 'plugin-docs',
      component: () => import('@/pages/PluginDocs.vue'),
      meta: { layout: 'app' },
    },

    // Settings — full-page, section-deep-linkable (/settings/:section).
    {
      path: '/settings/:section?',
      name: 'settings',
      component: () => import('@/pages/Settings.vue'),
      meta: { layout: 'app' },
    },
  ],
})

// Global access guard:
// 1. A gated (pending_guardian) profile can only see the blank-layout
//    auth pages and its gate screen — every app route redirects there.
// 2. `requiresInstructorMode` routes need an instructor account with
//    instructor mode active (mode flips are cheap, so send the user
//    home rather than flipping silently).
// 3. `requiresRole` routes (e.g. the guardian dashboard) are only for
//    that account role.
router.beforeEach(async (to) => {
  const openNames = new Set(['onboarding', 'profiles', 'guardian-gate'])
  if (openNames.has(String(to.name))) return true

  const { useProfiles } = await import('@/composables/useProfiles')
  const { useAccountStatus } = await import('@/composables/useAccountStatus')
  const { isUnlocked } = useProfiles()
  if (!isUnlocked.value) return true // App.vue handles onboarding/picker routing

  const { loaded, refreshAccountStatus, isPendingGuardian, role } = useAccountStatus()
  if (!loaded.value) await refreshAccountStatus()
  if (isPendingGuardian.value) return { name: 'guardian-gate' }

  if (to.meta.requiresInstructorMode) {
    const { useMode } = await import('@/composables/useMode')
    if (!useMode().isInstructorMode.value) return { name: 'home' }
  }

  if (to.meta.requiresRole && to.meta.requiresRole !== role.value) {
    return { name: 'home' }
  }

  // Parents have no learner home; route them to their dashboard.
  if (to.name === 'home' && role.value === 'parent') {
    return { path: '/guardian' }
  }

  return true
})

export default router
