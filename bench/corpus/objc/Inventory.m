#import "Inventory.h"

static NSString *const CRPErrorDomain = @"corpus.inventory";

@interface CRPInventory ()
@property (nonatomic, strong) NSMutableDictionary<NSString *, NSNumber *> *storage;
@property (nonatomic) NSUInteger limit;
@end

@implementation CRPInventory

- (instancetype)initWithLimit:(NSUInteger)limit {
    if ((self = [super init])) {
        _storage = [NSMutableDictionary dictionary];
        _limit = limit;
        _status = CRPStatusPending;
    }
    return self;
}

- (NSDictionary<NSString *, NSNumber *> *)items {
    return [self.storage copy];
}

- (BOOL)addSku:(NSString *)sku quantity:(NSInteger)quantity error:(NSError **)error {
    if (sku.length == 0 || self.storage.count >= self.limit) {
        if (error) {
            *error = [NSError errorWithDomain:CRPErrorDomain
                                         code:1
                                     userInfo:@{NSLocalizedDescriptionKey : @"cannot add «sku»"}];
        }
        return NO;
    }
    NSInteger current = [self.storage[sku] integerValue];
    self.storage[sku] = @(current + quantity);
    [self.observer inventoryDidChange:self];
    return YES;
}

- (void)enumerateUsingBlock:(void (^)(NSString *, NSInteger, BOOL *))block {
    __block BOOL stop = NO;
    [self.storage enumerateKeysAndObjectsUsingBlock:^(NSString *key, NSNumber *obj, BOOL *innerStop) {
        block(key, obj.integerValue, &stop);
        *innerStop = stop;
    }];
}

- (id)copyWithZone:(NSZone *)zone {
    CRPInventory *copy = [[[self class] allocWithZone:zone] initWithLimit:self.limit];
    copy.storage = [self.storage mutableCopy];
    return copy;
}

@end
