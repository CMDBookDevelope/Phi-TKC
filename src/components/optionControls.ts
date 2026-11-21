import { defineComponent, h, useId, type Component } from 'vue';
import { VCombobox, VSelect, VSwitch, VTextField } from 'vuetify/components';
import { isAndroid } from '@/platform';
import './optionControls.css';

function withOptionLabel<T extends Component>(control: T, toggle = false): T {
  return defineComponent({
    inheritAttrs: false,
    props: { label: String },
    setup(props, { attrs, slots }) {
      const android = isAndroid();
      const generatedId = useId();
      return () => {
        if (android) return h(control, { ...attrs, label: props.label }, slots);
        const { class: classes, style, hint: _hint, persistentHint: _persistentHint, 'persistent-hint': _legacyHint, ...inputAttrs } = attrs;
        const id = String(inputAttrs.id ?? `option-${generatedId}`);
        const labelId = `${id}-label`;
        return h('div', { class: ['option-field', { 'option-field--toggle': toggle }, classes], style }, [
          props.label && h('label', { class: 'option-field__label', id: labelId, for: id }, props.label),
          h(control, {
            ...inputAttrs,
            id,
            ...(inputAttrs.rules && !toggle ? { hideDetails: 'auto' } : {}),
            ...(props.label ? { 'aria-labelledby': labelId } : {}),
            class: 'option-field__control',
          }, slots),
        ]);
      };
    },
  }) as unknown as T;
}

export const OptionSelect = withOptionLabel(VSelect);
export const OptionCombobox = withOptionLabel(VCombobox);
export const OptionTextField = withOptionLabel(VTextField);
export const OptionSwitch = withOptionLabel(VSwitch, true);
