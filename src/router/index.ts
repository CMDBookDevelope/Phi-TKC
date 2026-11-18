import { nextTick } from 'vue';
import { createRouter, createWebHistory } from 'vue-router';

import { setTitle } from '../common';
import { i18n } from '../i18n';

import AboutView from '../AboutView.vue';
import RenderView from '../RenderView.vue';
import RPEView from '../RPEView.vue';
import TasksView from '../TasksView.vue';
import BatchRenderView from '../BatchRenderView.vue';
import Setting from '../Setting.vue';

import { isMobile } from '../platform';

const router = createRouter({
  history: createWebHistory(import.meta.env.BASE_URL),
  routes: [
    { path: '/', name: 'render', component: RenderView },
    { path: '/rpe', name: 'rpe', component: RPEView,
      beforeEnter: () => isMobile() ? { name: 'render', replace: true } : true },
    {
      path: '/tasks',
      name: 'tasks',
      component: TasksView,
    },
    {
      path: '/batch',
      name: 'batch-render',
      component: BatchRenderView,
      meta: { title: 'Batch Render' },
    },
    {
      path: '/setting',
      name: 'setting',
      component: Setting,
    },
    {
      path: '/about',
      name: 'about',
      component: AboutView,
    },
  ],
  // Views own their content scrollers; the application shell never scrolls.
  scrollBehavior() {
    return false;
  },
});

router.afterEach((to) => {
  nextTick(() => {
    const title = i18n.global.t('title-' + ((to.name as string | undefined | null) || 'default'));
    setTitle(title);
  });
});

export default router;
