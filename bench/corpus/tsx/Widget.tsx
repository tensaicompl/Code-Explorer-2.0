import React, { useCallback, useMemo, useState } from "react";

export interface WidgetProps<T> {
  items: readonly T[];
  render: (item: T, index: number) => React.ReactNode;
  title?: string;
  onSelect?: (item: T) => void;
}

/** A generic list widget: JSX, hooks, generics and fragments. */
export function Widget<T extends { id: string }>({
  items,
  render,
  title = "Liste — éléments",
  onSelect,
}: WidgetProps<T>): JSX.Element {
  const [selected, setSelected] = useState<string | null>(null);
  const sorted = useMemo(() => [...items].sort((a, b) => a.id.localeCompare(b.id)), [items]);

  const select = useCallback(
    (item: T) => {
      setSelected(item.id);
      onSelect?.(item);
    },
    [onSelect],
  );

  return (
    <section className="widget" aria-label={title}>
      <h2>{title}</h2>
      {sorted.length === 0 ? (
        <p className="empty">Nothing here.</p>
      ) : (
        <ul>
          {sorted.map((item, i) => (
            <li
              key={item.id}
              className={item.id === selected ? "selected" : undefined}
              onClick={() => select(item)}
            >
              {render(item, i)}
            </li>
          ))}
        </ul>
      )}
      <>
        <small>{`${sorted.length} items`}</small>
      </>
    </section>
  );
}

export default Widget;
