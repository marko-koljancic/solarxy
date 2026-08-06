// The app's dropdown.
//
// Every picker in the chrome used to be a native `<select>`. There was no
// `select` styling in styles.css at all -- no `appearance: none`, no chevron --
// so they rendered as OS controls: system-grey on the warm cream light theme,
// system-blue focus rings against an amber accent, and a popup list drawn by
// the platform that no token could reach.
//
// The interaction model is lifted from PaneToolbar's ghost menus, which
// already got this right: click to open, close on outside pointerdown or
// Escape, a tick against the current value. This adds what a value-bound
// control needs beyond a menu -- typeahead, Home/End, and arrow keys that move
// a highlight without committing until Enter.
//
// Portaled by default: the list renders into the shared dropdown layer,
// which positions against the trigger, flips upward at the viewport edge,
// and paints above every panel and modal (the dropdown layer sits above
// the modal layer). The inline mode this component started with clipped
// against every scrolling host: a dropdown on the last row of the
// Properties panel opened into the panel's scroll overflow and had to be
// scrolled to. DropdownPortal owns positioning and the whole dismiss
// contract in this mode.
//
// `portal={false}` opts back into the inline absolute list for the one
// host shape the portal cannot serve: a Select nested inside another
// portaled panel, where the child list rendering outside the parent's
// DOM would read as an outside click and dismiss the parent.

import { useEffect, useId, useMemo, useRef, useState } from "react";
import { IconCheck, IconChevronDown } from "../icons";
import { DropdownPortal } from "./DropdownPortal";
import { claimEscape, releaseEscape } from "./escapeClaim";

export interface SelectOption<T extends string> {
  value: T;
  label: string;
  /** Optional right-aligned hint (units, a px figure, a category). */
  hint?: string;
  /** Optional leading swatch: any CSS background value (a color, a
   * linear-gradient). Rendered as a small chip before the label and in
   * the closed trigger. */
  swatch?: string;
  disabled?: boolean;
}

interface SelectProps<T extends string> {
  value: T;
  options: readonly SelectOption<T>[];
  onChange: (value: T) => void;
  /** Accessible name. Required: these replace `<select>` elements that were
   * labelled by a `<Row label=...>` the control could not see. */
  ariaLabel: string;
  /** Fixed width, where a call site relied on one. */
  width?: number | string;
  disabled?: boolean;
  id?: string;
  /** Render the list into the shared dropdown layer (the default). Pass
   * false only when this Select nests inside another portaled panel,
   * whose dismiss contract would treat the portaled child list as an
   * outside click. */
  portal?: boolean;
}

export function Select<T extends string>({
  value,
  options,
  onChange,
  ariaLabel,
  width,
  disabled,
  id,
  portal = true,
}: SelectProps<T>) {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const rootRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const typeahead = useRef({ buffer: "", at: 0 });
  const listId = useId();

  const selectedIndex = useMemo(
    () => Math.max(0, options.findIndex((o) => o.value === value)),
    [options, value],
  );
  const current = options[selectedIndex];

  useEffect(() => {
    if (open) setActive(selectedIndex);
  }, [open, selectedIndex]);

  // An open list owns Escape, so closing it never closes the dialog around
  // it. The handler below is `onKeyDown` on the trigger -- the target phase,
  // which a modal's capture-phase listener beats outright. See `escapeClaim`.
  useEffect(() => {
    if (!open) return undefined;
    claimEscape();
    return releaseEscape;
  }, [open]);

  // Inline only: a portaled list lives outside `rootRef`, so this would read
  // every option click as an outside click. DropdownPortal owns dismissal
  // (outside pointerdown, Escape, resize) in that mode.
  useEffect(() => {
    if (!open || portal) return;
    const onDown = (e: PointerEvent) => {
      if (!(e.target instanceof Element) || !rootRef.current?.contains(e.target)) setOpen(false);
    };
    window.addEventListener("pointerdown", onDown, true);
    return () => window.removeEventListener("pointerdown", onDown, true);
  }, [open, portal]);

  // Keep the highlighted row in view when arrowing through a long list.
  useEffect(() => {
    if (!open) return;
    listRef.current?.querySelector(`[data-i="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }, [open, active]);

  const commit = (i: number) => {
    const opt = options[i];
    if (!opt || opt.disabled) return;
    onChange(opt.value);
    setOpen(false);
  };

  /** Jump to the next option starting with the typed run, as a native select
   * does. Repeating one letter cycles matches; typing a run seeks the run. */
  const seek = (key: string) => {
    const now = Date.now();
    const t = typeahead.current;
    t.buffer = now - t.at > 700 ? key : t.buffer + key;
    t.at = now;
    const q = t.buffer.toLowerCase();
    const from = t.buffer.length === 1 ? (open ? active : selectedIndex) + 1 : 0;
    for (let n = 0; n < options.length; n++) {
      const i = (from + n) % options.length;
      const o = options[i];
      if (!o.disabled && o.label.toLowerCase().startsWith(q)) {
        if (open) setActive(i);
        else commit(i);
        return;
      }
    }
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (disabled) return;
    const step = (delta: number) => {
      e.preventDefault();
      if (!open) {
        setOpen(true);
        return;
      }
      setActive((a) => {
        // Skip disabled rows rather than parking the highlight on one.
        for (let n = 1; n <= options.length; n++) {
          const i = (a + delta * n + options.length * n) % options.length;
          if (!options[i].disabled) return i;
        }
        return a;
      });
    };

    switch (e.key) {
      case "ArrowDown":
      case "Down":
        step(1);
        break;
      case "ArrowUp":
      case "Up":
        step(-1);
        break;
      case "Home":
        e.preventDefault();
        setActive(0);
        break;
      case "End":
        e.preventDefault();
        setActive(options.length - 1);
        break;
      case "Enter":
      case " ":
        e.preventDefault();
        if (open) commit(active);
        else setOpen(true);
        break;
      case "Escape":
        if (open) {
          e.preventDefault();
          e.stopPropagation();
          setOpen(false);
        }
        break;
      case "Tab":
        // Never swallow Tab: it is the node palette's binding, and stealing
        // focus-nav from a closed control would be worse still.
        setOpen(false);
        break;
      default:
        if (e.key.length === 1 && !e.metaKey && !e.ctrlKey && !e.altKey) seek(e.key);
    }
  };

  const list = (
    <div
      ref={listRef}
      id={listId}
      className={`select-list${portal ? " portaled" : ""}`}
      role="listbox"
      tabIndex={-1}
    >
      {options.map((o, i) => (
        <button
          key={o.value}
          type="button"
          data-i={i}
          role="option"
          aria-selected={o.value === value}
          disabled={o.disabled}
          className={`select-option${i === active ? " active" : ""}`}
          // pointerdown, not click: the outside-pointerdown listener runs
          // in the capture phase and would close the list first.
          onPointerDown={(e) => {
            e.preventDefault();
            commit(i);
          }}
          onPointerEnter={() => setActive(i)}
        >
          <span className="select-check">{o.value === value && <IconCheck size={11} />}</span>
          {o.swatch && <span className="select-swatch" style={{ background: o.swatch }} />}
          <span className="select-option-label">{o.label}</span>
          {o.hint && <span className="select-option-hint">{o.hint}</span>}
        </button>
      ))}
    </div>
  );

  return (
    <div ref={rootRef} className="select" style={width ? { width } : undefined}>
      <button
        id={id}
        type="button"
        className={`select-trigger${open ? " open" : ""}`}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={ariaLabel}
        aria-controls={open ? listId : undefined}
        disabled={disabled}
        onClick={() => setOpen((o) => !o)}
        onKeyDown={onKeyDown}
      >
        {current?.swatch && <span className="select-swatch" style={{ background: current.swatch }} />}
        <span className="select-value">{current?.label ?? value}</span>
        <IconChevronDown size={12} />
      </button>
      {open &&
        (portal ? (
          <DropdownPortal anchorRef={rootRef} onClose={() => setOpen(false)}>
            {list}
          </DropdownPortal>
        ) : (
          list
        ))}
    </div>
  );
}
