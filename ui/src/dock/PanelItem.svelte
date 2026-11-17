<script lang="ts">
  import DockItem from '$lib/components/DockItem.svelte';
  import type { DockItem as DockItemModel } from '$lib/ipc/generated/DockItem';
  import type { DockState } from '$lib/ipc/generated/DockState';

  import type { DockController } from './controller.svelte';

  interface Props {
    item: DockItemModel;
    /** The snapshot the item is from: the battery, volume and clock items read it. */
    state: DockState;
    /** The dock page's parts, whose state decides how the item is drawn. */
    dock: DockController;
  }

  let { item, state, dock }: Props = $props();
</script>

<!-- One app or fixed item of the panel, with the dock's state for it. -->
<DockItem
  {item}
  iconUrl={item.iconKey !== null
    ? dock.icons.iconUrlAt(
        item.iconKey,
        dock.icons.iconSizes.rest,
        dock.icons.previousIconSizes?.rest,
      )
    : null}
  magnifiedIconUrl={item.iconKey !== null && dock.icons.dualSources
    ? dock.icons.iconUrlAt(
        item.iconKey,
        dock.icons.iconSizes.magnified,
        dock.icons.previousIconSizes?.magnified,
      )
    : null}
  showDots={dock.view.showDots}
  tooltip={dock.tooltip.tooltipKey === item.key ||
    dock.keys.focusTooltipKey === item.key ||
    (dock.files.dropTargetKey === item.key && dock.view.tooltipsEnabled)}
  dropTarget={dock.files.dropTargetKey === item.key}
  power={state.power}
  volume={state.volume}
  clockFormat={state.clockFormat}
  bounce={dock.view.bounceEnabled && dock.store.bounceKey === item.key
    ? dock.store.bounceSerial
    : 0}
  animate={dock.view.itemMotion}
  enter={dock.introsEnabled && item.key !== dock.landing.landedKey}
  lifted={dock.reorder.liftedKey === item.key}
  edge={dock.view.edge}
  tabStop={dock.keys.tabStop === item.key}
  onprimary={dock.intents.onprimary}
  onmiddle={dock.intents.onmiddle}
  oncontext={dock.intents.oncontext}
  onhover={dock.tooltip.onhover}
  onpress={dock.reorderPointer.onpress}
  onsettled={dock.onItemSettled}
/>
