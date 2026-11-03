<script lang="ts">
  // Windows 11 slider on a native range input, with the value shown beside
  // it (a label rather than a hover tooltip).
  interface Props {
    id: string;
    min: number;
    max: number;
    step: number;
    value: number;
    disabled?: boolean;
    /** Text for the value label and `aria-valuetext`. */
    format: (value: number) => string;
    onchange: (value: number) => void;
  }

  let { id, min, max, step, value, disabled = false, format, onchange }: Props = $props();

  const percent = $derived(max > min ? ((value - min) / (max - min)) * 100 : 0);
  const text = $derived(format(value));
</script>

<span class="slider" class:disabled>
  <input
    type="range"
    {id}
    {min}
    {max}
    {step}
    {value}
    {disabled}
    aria-valuetext={text}
    style:--percent="{percent}%"
    oninput={(e) => onchange(Number(e.currentTarget.value))}
  />
  <output for={id}>{text}</output>
</span>

<style>
  .slider {
    display: inline-flex;
    align-items: center;
    gap: 12px;
  }

  output {
    min-width: 52px;
    color: var(--text);
    font-size: 14px;
    font-variant-numeric: tabular-nums;
    text-align: right;
  }

  .disabled output {
    color: var(--text-disabled);
  }

  input {
    appearance: none;
    -webkit-appearance: none;
    width: 200px;
    height: 20px;
    margin: 0;
    background: transparent;
    cursor: default;
  }

  input::-webkit-slider-runnable-track {
    height: 4px;
    border-radius: 2px;
    background: linear-gradient(
      to right,
      var(--accent) 0 var(--percent),
      var(--track) var(--percent) 100%
    );
  }

  input::-webkit-slider-thumb {
    appearance: none;
    -webkit-appearance: none;
    box-sizing: border-box;
    width: 20px;
    height: 20px;
    margin-top: -8px;
    border: 1px solid var(--control-border-bottom);
    border-radius: 50%;
    background: radial-gradient(circle, var(--accent) 0 5.5px, var(--thumb-bg) 6px);
    transition: background 100ms ease;
  }

  input:hover:not(:disabled)::-webkit-slider-thumb {
    background: radial-gradient(circle, var(--accent) 0 7px, var(--thumb-bg) 7.5px);
  }

  input:active:not(:disabled)::-webkit-slider-thumb {
    background: radial-gradient(circle, var(--accent) 0 5px, var(--thumb-bg) 5.5px);
  }

  input:disabled {
    opacity: 0.45;
  }

  input:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: 2px;
    border-radius: 4px;
  }

  @media (prefers-reduced-motion: reduce) {
    input::-webkit-slider-thumb {
      transition: none;
    }
  }
</style>
