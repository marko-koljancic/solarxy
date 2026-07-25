// One top-level menu with its dropdown: hover-open, nested submenus,
// dividers, checkmarks, disabled items, and right-aligned shortcut hints.
// Ported from the Minimystix Header/MenuItem pair to the token system.

import { useState, type ReactNode } from "react";
import { IconCheck, IconChevronRight } from "../../icons";

export interface SubMenuEntry {
  label: string;
  onClick: () => void;
  checked?: boolean;
  disabled?: boolean;
  shortcut?: string;
  /** Small leading icon (e.g. the node glyph in the Add menu). */
  icon?: ReactNode;
}

export interface MenuEntry {
  label?: string;
  onClick?: () => void;
  divider?: boolean;
  checked?: boolean;
  disabled?: boolean;
  shortcut?: string;
  submenu?: SubMenuEntry[];
}

interface MenuItemProps {
  title: string;
  entries: MenuEntry[];
}

export function MenuItem({ title, entries }: MenuItemProps) {
  const [open, setOpen] = useState(false);
  const [hoveredSubmenu, setHoveredSubmenu] = useState<number | null>(null);

  const close = () => {
    setOpen(false);
    setHoveredSubmenu(null);
  };

  return (
    <div
      className="menu-item"
      onMouseEnter={() => setOpen(true)}
      onMouseLeave={close}
      onKeyDown={(e) => {
        if (e.key === "Escape") close();
      }}
    >
      {title}
      {open && (
        <div className="menu-dropdown" role="menu">
          {entries.map((entry, i) =>
            entry.divider ? (
              <div key={i} className="menu-divider" />
            ) : (
              <div
                key={i}
                role="menuitem"
                className={`menu-entry${entry.disabled ? " disabled" : ""}`}
                onClick={() => {
                  if (entry.disabled || entry.submenu) return;
                  entry.onClick?.();
                  close();
                }}
                onMouseEnter={() => setHoveredSubmenu(entry.submenu ? i : null)}
              >
                <span className="menu-check">{entry.checked && <IconCheck size={12} />}</span>
                <span className="menu-label">{entry.label}</span>
                {entry.shortcut && <span className="menu-shortcut">{entry.shortcut}</span>}
                {entry.submenu && (
                  <span className="menu-caret">
                    <IconChevronRight size={12} />
                  </span>
                )}
                {entry.submenu && hoveredSubmenu === i && (
                  <div className="menu-submenu">
                    {entry.submenu.map((sub, j) => (
                      <div
                        key={j}
                        role="menuitem"
                        className={`menu-entry${sub.disabled ? " disabled" : ""}`}
                        onClick={(e) => {
                          e.stopPropagation();
                          if (sub.disabled) return;
                          sub.onClick();
                          close();
                        }}
                      >
                        <span className="menu-check">{sub.checked && <IconCheck size={12} />}</span>
                        {sub.icon && <span className="menu-icon">{sub.icon}</span>}
                        <span className="menu-label">{sub.label}</span>
                        {sub.shortcut && <span className="menu-shortcut">{sub.shortcut}</span>}
                      </div>
                    ))}
                  </div>
                )}
              </div>
            ),
          )}
        </div>
      )}
    </div>
  );
}
