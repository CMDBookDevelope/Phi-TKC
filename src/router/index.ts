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

import { useOnLoaded } from '../appState';

const onLoaded = useOnLoaded();

const router = createRouter({
  history: createWebHistory(import.meta.env.BASE_URL),
  routes: [
    { path: '/', name: 'render', component: RenderView },
    { path: '/rpe', name: 'rpe', component: RPEView },
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
  scrollBehavior(_to, _from, savedPosition) {
    return new Promise((resolve) => {
      let settled = false;
      const finish = () => {
        if (settled) return;
        settled = true;
        resolve(savedPosition ? savedPosition : { top: 0 });
      };
      onLoaded.value = finish;
      // Navigation between two routes that reuse the same component instance does not
      // change `routeComponent`, so don't leave the scroll promise pending forever.
      setTimeout(finish, 600);
    });
  },
});

router.afterEach((to) => {
  nextTick(() => {
    const title = i18n.global.t('title-' + ((to.name as string | undefined | null) || 'default'));
    setTitle(title);
  });
});

export default router;
