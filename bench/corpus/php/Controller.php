<?php

declare(strict_types=1);

namespace Corpus\Http;

use Corpus\Model\{Order, Line};
use InvalidArgumentException;

#[\Attribute(\Attribute::TARGET_METHOD)]
final class Route
{
    public function __construct(public string $path, public string $method = 'GET') {}
}

enum Status: string
{
    case Pending = 'pending';
    case Done = 'done';

    public function label(): string
    {
        return match ($this) {
            self::Pending => 'En attente',
            self::Done => 'Terminé',
        };
    }
}

interface Responder
{
    public function respond(array $payload, int $status = 200): string;
}

trait Logs
{
    private array $log = [];

    protected function note(string $message, mixed ...$args): void
    {
        $this->log[] = sprintf($message, ...$args);
    }
}

abstract class Base implements Responder
{
    use Logs;

    public function respond(array $payload, int $status = 200): string
    {
        $this->note('responding %d', $status);
        return json_encode(['status' => $status, 'data' => $payload], JSON_THROW_ON_ERROR);
    }
}

final class OrderController extends Base
{
    public function __construct(private readonly array $orders = []) {}

    #[Route('/orders/{id}')]
    public function show(string $id): string
    {
        $order = $this->orders[$id] ?? throw new InvalidArgumentException("no order {$id}");
        $total = array_reduce($order['lines'] ?? [], fn ($sum, $line) => $sum + $line['qty'], 0);
        return $this->respond(['id' => $id, 'total' => $total, 'status' => Status::Pending->label()]);
    }

    public function report(): string
    {
        $body = <<<EOT
        Report for {$this->count()} orders
          generated with "quotes" and \$escaped dollars
        EOT;
        $raw = <<<'RAW'
        raw {$not} interpolated
        RAW;
        return $body . $raw;
    }

    private function count(): int
    {
        return count($this->orders);
    }

    public static function make(iterable $source): static
    {
        $orders = [];
        foreach ($source as $key => ['id' => $id, 'lines' => $lines]) {
            $orders[$id ?? $key] = ['lines' => $lines];
        }
        return new static($orders);
    }
}
