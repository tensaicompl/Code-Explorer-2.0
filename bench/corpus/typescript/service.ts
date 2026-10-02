import { EventEmitter } from "events";
import type { Readable } from "stream";

/** Interfaces, generics, enums, decorators-free classes, async and namespaces. */
export enum Status {
  Pending = "pending",
  Done = "done",
  Failed = 3,
}

export interface Entity<Id extends string | number = string> {
  readonly id: Id;
  createdAt?: Date;
}

export interface Order extends Entity {
  lines: Array<{ sku: string; qty: number }>;
  status: Status;
  notes: string | null;
}

export type Handler<T> = (value: T, index: number) => Promise<void> | void;

export abstract class Repository<T extends Entity> {
  protected readonly items = new Map<T["id"], T>();

  abstract validate(item: T): boolean;

  async save(item: T): Promise<T> {
    if (!this.validate(item)) {
      throw new TypeError(`invalid item ${String(item.id)}`);
    }
    this.items.set(item.id, item);
    return item;
  }

  find(id: T["id"]): T | undefined {
    return this.items.get(id);
  }
}

export class OrderService extends Repository<Order> {
  private readonly events = new EventEmitter();

  constructor(private readonly limit: number = 100) {
    super();
  }

  validate(order: Order): boolean {
    return order.lines.length > 0 && order.lines.length <= this.limit;
  }

  on(event: "saved", handler: Handler<Order>): this {
    this.events.on(event, handler);
    return this;
  }

  async complete(id: string): Promise<Status> {
    const order = this.find(id);
    if (!order) return Status.Failed;
    order.status = Status.Done;
    await this.save(order);
    this.events.emit("saved", order);
    return order.status;
  }

  *pending(): Generator<Order, void, unknown> {
    for (const order of this.items.values()) {
      if (order.status === Status.Pending) yield order;
    }
  }
}

export namespace Totals {
  export function quantity(order: Order): number {
    return order.lines.reduce((sum, { qty }) => sum + qty, 0);
  }

  export const summary = (orders: readonly Order[]): Record<Status, number> => {
    const counts = { [Status.Pending]: 0, [Status.Done]: 0, [Status.Failed]: 0 } as Record<Status, number>;
    orders.forEach((o) => counts[o.status]++);
    return counts;
  };
}

export async function drain(stream: Readable): Promise<string> {
  const chunks: string[] = [];
  for await (const chunk of stream) {
    chunks.push(typeof chunk === "string" ? chunk : chunk.toString("utf8"));
  }
  return chunks.join("") + "é\n";
}
