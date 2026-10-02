import Foundation

/// Protocols, generics with constraints, enums with payloads, extensions and closures.
protocol Identifiable {
    associatedtype ID: Hashable
    var id: ID { get }
}

enum Event<Payload> {
    case created(Payload)
    case deleted(id: String)
    case batch([Event<Payload>])
}

struct Item: Identifiable, Codable, Equatable {
    let id: String
    var quantity: Int
    var price: Decimal = 0
}

final class Store<Element: Identifiable> {
    private var items: [Element.ID: Element] = [:]
    private(set) var history: [String] = []

    subscript(id: Element.ID) -> Element? {
        get { items[id] }
        set { items[id] = newValue }
    }

    func insert(_ element: Element, onInsert: ((Element) -> Void)? = nil) {
        items[element.id] = element
        history.append("inserted \(element.id)")
        onInsert?(element)
    }

    func all(where predicate: (Element) throws -> Bool) rethrows -> [Element] {
        try items.values.filter(predicate)
    }
}

extension Store where Element == Item {
    var totalQuantity: Int { items.values.reduce(0) { $0 + $1.quantity } }

    func describe(_ event: Event<Item>) -> String {
        switch event {
        case .created(let item) where item.quantity == 0:
            return "created empty \(item.id)"
        case .created(let item):
            return "created \(item.id) × \(item.quantity)"
        case .deleted(let id):
            return "deleted \(id)"
        case .batch(let events):
            return events.map(describe).joined(separator: "\n")
        }
    }
}

enum InventoryError: Error {
    case missing(String)
    case invalid(reason: String)
}

func load(_ json: String) throws -> [Item] {
    guard let data = json.data(using: .utf8) else {
        throw InventoryError.invalid(reason: "not UTF-8")
    }
    return try JSONDecoder().decode([Item].self, from: data)
}

let store = Store<Item>()
store.insert(Item(id: "café", quantity: 2)) { print("added \($0.id)") }
let multiline = """
    Inventaire — \(store.totalQuantity) unités
    "quoted" and \\escaped
    """
