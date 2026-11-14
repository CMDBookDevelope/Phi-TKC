import { defineComponent, h, mergeProps, onMounted, onUpdated, ref, useAttrs, watch, type App, type ComponentPublicInstance } from 'vue';
import { useTheme } from 'vuetify';
import { VProgressCircular, VProgressLinear } from 'vuetify/components';

// Keep Vuetify's geometry, defaults, events and slots. Only bake the track's
// opacity into its paint, avoiding a separate opacity layer over backdrop blur.
function trackPaint(color: string, opacity: string): string {
  const alpha = Math.max(0, Math.min(1, Number(opacity)));
  const match = color.match(/^rgba?\(([^)]+)\)$/);
  if (match) {
    const channels = match[1].replace(/\//g, ' ').split(/[,\s]+/).filter(Boolean);
    const value = (channel: string, scale: number) => channel.endsWith('%')
      ? parseFloat(channel) * scale / 100 : Number(channel);
    if (channels.length >= 3) {
      const [r, g, b] = channels.slice(0, 3).map(channel => value(channel, 255));
      const sourceAlpha = channels[3] == null ? 1 : value(channels[3], 1);
      return `rgba(${r}, ${g}, ${b}, ${sourceAlpha * alpha})`;
    }
  }
  // Modern CSS color spaces retain their original gamut instead of being
  // converted to sRGB. RGB/HEX/theme colors above also work on older WebKit.
  return `color-mix(in srgb, ${color} ${alpha * 100}%, transparent)`;
}

function paintProgress(root: HTMLElement, circular: boolean) {
  const paintClass = circular ? 'tk-progress-circular--paint' : 'tk-progress-linear--paint';
  // Read Vuetify's resolved paint, including defaults, palette names, custom
  // colors and theme border alpha. This class is restored before the next paint.
  root.classList.remove(paintClass);
  const selectors = circular
    ? [['.v-progress-circular__underlay', '--tk-progress-underlay', 'stroke']]
    : [
        ['.v-progress-linear__background', '--tk-progress-background', 'backgroundColor'],
        ['.v-progress-linear__buffer', '--tk-progress-buffer', 'backgroundColor'],
        ['.v-progress-linear__stream', '--tk-progress-stream', 'color'],
      ];
  for (const [selector, variable, property] of selectors) {
    const track = root.querySelector(selector);
    if (!track) continue;
    const style = getComputedStyle(track);
    root.style.setProperty(variable, trackPaint(style[property as 'color'], style.opacity));
  }
  root.classList.add(paintClass);
}

function progressComponent(circular: boolean) {
  const Original = circular ? VProgressCircular : VProgressLinear;
  const paintClass = circular ? 'tk-progress-circular--paint' : 'tk-progress-linear--paint';
  return defineComponent({
    name: circular ? 'AppProgressCircular' : 'AppProgressLinear',
    inheritAttrs: false,
    setup(_, { slots }) {
      const attrs = useAttrs();
      const theme = useTheme();
      const original = ref<{ $el: HTMLElement }>();
      const painted = ref(false);
      let lastSignature = '';

      function signature() {
        // Progress polling changes modelValue very often. It must not trigger
        // a computed-style read or repaint the track on every frame.
        return JSON.stringify([
          attrs.color, attrs.bgColor, attrs['bg-color'], attrs.bgOpacity, attrs['bg-opacity'],
          attrs.bufferColor, attrs['buffer-color'], attrs.bufferOpacity, attrs['buffer-opacity'],
          attrs.theme, attrs.class, attrs.style, attrs.tag, attrs.stream,
        ]);
      }

      function updatePaint() {
        const root = original.value?.$el;
        if (!(root instanceof HTMLElement)) return;
        paintProgress(root, circular);
        painted.value = true;
        lastSignature = signature();
      }

      onMounted(updatePaint);
      onUpdated(() => {
        if (signature() !== lastSignature) updatePaint();
      });
      watch(() => theme.themes.value, updatePaint, { deep: true, flush: 'post' });
      watch(() => theme.name.value, updatePaint, { flush: 'post' });

      return () => h(Original as typeof VProgressLinear, mergeProps(attrs, {
        ref: original,
        class: { [paintClass]: painted.value },
      }), slots);
    },
  });
}

export const AppProgressLinear = progressComponent(false);
export const AppProgressCircular = progressComponent(true);

// Vuetify imports the original components inside VBtn and VField, bypassing
// global component aliases. Apply the same paint there while retaining their
// original loader props and render functions.
export function installInternalProgressPaint(app: App) {
  const controls = new WeakMap<object, { update: () => void; paintClass: string }>();
  app.mixin({
    mounted(this: ComponentPublicInstance) {
      const name = this.$options.name;
      if (name !== 'VProgressLinear' && name !== 'VProgressCircular') return;
      if (this.$parent?.$options.name?.startsWith('AppProgress')) return;
      const circular = name === 'VProgressCircular';
      const paintClass = circular ? 'tk-progress-circular--paint' : 'tk-progress-linear--paint';
      const update = () => {
        if (this.$el instanceof HTMLElement) paintProgress(this.$el, circular);
      };
      controls.set(this, { update, paintClass });
      const theme = useTheme();
      const paintProps = ['color', 'bgColor', 'bgOpacity', 'bufferColor', 'bufferOpacity', 'theme', 'class', 'style', 'tag', 'stream'];
      this.$watch(() => paintProps.map(key => (this.$props as Record<string, unknown>)[key]), update, { deep: true, flush: 'post' });
      watch(() => theme.themes.value, update, { deep: true, flush: 'post' });
      watch(() => theme.name.value, update, { flush: 'post' });
      update();
    },
    updated(this: ComponentPublicInstance) {
      const control = controls.get(this);
      // A native loader's class update can replace its imperatively added
      // marker. Normal progress updates keep it and do no style reads.
      if (control && this.$el instanceof HTMLElement && !this.$el.classList.contains(control.paintClass)) control.update();
    },
  });
}
