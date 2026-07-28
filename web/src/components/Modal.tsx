// The shared dialog shell: backdrop, a draggable titlebar, a resizable
// body, and the per-dialog Esc/backdrop semantics the hand-rolled modals
// used to each own. Content markup stays with each dialog; this component
// owns only chrome and behavior, so migrating a dialog is mechanical
// (drop its backdrop wrapper, its h3, and its Esc effect).

import { useEffect, type ReactNode } from "react";
import { useDragResize } from "../hooks/useDragResize";
import { isEscapeClaimed } from "./escapeClaim";

export function Modal({
  id,
  title,
  onClose,
  children,
  className,
  closeOnEsc = true,
  closeOnBackdrop = true,
  resizable = true,
  bodyLayout = "block",
  minWidth,
  minHeight,
}: {
  /** Session size-memory key; omit to forget bounds on close. */
  id?: string;
  title: ReactNode;
  /** Omit for dialogs that only close through explicit actions
   * (RecoveryPrompt): no Esc, no backdrop close, no X. */
  onClose?: () => void;
  children: ReactNode;
  /** Extra classes on the modal box (`modal-wide`, `modal-prefs`, ...). */
  className?: string;
  closeOnEsc?: boolean;
  closeOnBackdrop?: boolean;
  resizable?: boolean;
  /** `"column"` makes the body a flex column, so a dialog with a fixed
   * height can give one child `flex: 1` and pin a footer to the bottom.
   *
   * Opt-in rather than the default because the body is a plain block for
   * good reason: ordinary prose dialogs size to their content. But a dialog
   * that sets its own `height` and expects `flex: 1` to work inside gets an
   * inert declaration and a footer stranded mid-box, which is exactly what
   * happened to Preferences, Screenshot and Turntable when they migrated
   * onto this shell. */
  bodyLayout?: "block" | "column";
  minWidth?: number;
  minHeight?: number;
}) {
  const { ref, style, headerProps, resizeProps } = useDragResize({ id, minWidth, minHeight });

  useEffect(() => {
    if (!onClose || !closeOnEsc) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        // A tooltip or dropdown open above this dialog owns Escape:
        // dismissing one must not also throw away the edits you opened it
        // to make. See `escapeClaim` for why the DOM will not do this.
        if (isEscapeClaimed()) return;
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose, closeOnEsc]);

  return (
    <div
      className="modal-backdrop"
      onClick={onClose && closeOnBackdrop ? onClose : undefined}
    >
      <div
        ref={ref}
        className={`modal${className ? ` ${className}` : ""}`}
        style={style}
        role="dialog"
        aria-modal="true"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-titlebar" {...headerProps}>
          <h3>{title}</h3>
          {onClose && (
            <button
              type="button"
              className="modal-close"
              aria-label="Close"
              onClick={onClose}
            >
              <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden>
                <path
                  d="M4 4l8 8M12 4l-8 8"
                  stroke="currentColor"
                  strokeWidth="1.5"
                  strokeLinecap="round"
                  fill="none"
                />
              </svg>
            </button>
          )}
        </div>
        <div className={`modal-body${bodyLayout === "column" ? " modal-body-column" : ""}`}>
          {children}
        </div>
        {resizable && <div className="modal-resize" {...resizeProps} aria-hidden />}
      </div>
    </div>
  );
}
