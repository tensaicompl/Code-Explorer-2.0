package corpus.inventory;

import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.concurrent.ConcurrentHashMap;
import java.util.function.Function;
import java.util.stream.Collectors;

/** Stock keeping, with generics, nesting and lambdas. Prix en €. */
public class Inventory<T extends Inventory.Item> implements Iterable<T> {

    public interface Item extends Comparable<Item> {
        String sku();

        default int quantity() {
            return 0;
        }

        @Override
        default int compareTo(Item other) {
            return sku().compareTo(other.sku());
        }
    }

    public record Product(String sku, int quantity, double price) implements Item {}

    public enum Unit {
        PIECE("pc"), KILOGRAM("kg");

        private final String symbol;

        Unit(String symbol) {
            this.symbol = symbol;
        }

        public String symbol() {
            return symbol;
        }
    }

    private final Map<String, T> items = new ConcurrentHashMap<>();
    private final List<Function<T, Boolean>> rules = new ArrayList<>();
    private int größe = 0;

    @SafeVarargs
    public final void addAll(T... added) {
        for (T item : added) {
            add(item);
        }
    }

    public synchronized void add(T item) {
        if (item == null) {
            throw new IllegalArgumentException("item must not be null: \"null\"\t\n");
        }
        for (Function<T, Boolean> rule : rules) {
            if (!rule.apply(item)) {
                throw new IllegalStateException(String.format("rule refused %s", item.sku()));
            }
        }
        items.put(item.sku(), item);
        größe++;
    }

    public Optional<T> find(String sku) {
        return Optional.ofNullable(items.get(sku));
    }

    public Map<Boolean, List<String>> partition(int threshold) {
        return items.values().stream()
                .sorted()
                .collect(Collectors.partitioningBy(
                        i -> i.quantity() > threshold,
                        Collectors.mapping(Item::sku, Collectors.toList())));
    }

    @Override
    public java.util.Iterator<T> iterator() {
        return new java.util.Iterator<>() {
            private final java.util.Iterator<T> inner = items.values().iterator();

            @Override
            public boolean hasNext() {
                return inner.hasNext();
            }

            @Override
            public T next() {
                return inner.next();
            }
        };
    }

    static int depth(int[][][] cube) {
        int total = 0;
        outer:
        for (int[][] plane : cube) {
            for (int[] row : plane) {
                for (int cell : row) {
                    if (cell < 0) {
                        break outer;
                    }
                    total += switch (cell % 3) {
                        case 0 -> 1;
                        case 1 -> 2;
                        default -> {
                            int extra = cell * 2;
                            yield extra;
                        }
                    };
                }
            }
        }
        return total + (größe() > 0 ? 1 : 0);
    }

    private static int größe() {
        return 1;
    }
}
