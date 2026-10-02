// Foundation's annotation macros, which a parser that does not expand macros has to
// recover from.
#import <Foundation/Foundation.h>

NS_ASSUME_NONNULL_BEGIN

typedef NS_ENUM(NSInteger, CRPStatus) {
    CRPStatusPending,
    CRPStatusDone,
};

@protocol CRPObserver <NSObject>
- (void)inventoryDidChange:(id)inventory;
@optional
- (void)inventoryWillReset:(id)inventory;
@end

@interface CRPInventory : NSObject <NSCopying>

@property (nonatomic, copy, readonly) NSDictionary<NSString *, NSNumber *> *items;
@property (nonatomic, weak, nullable) id<CRPObserver> observer;
@property (nonatomic) CRPStatus status;

- (instancetype)initWithLimit:(NSUInteger)limit NS_DESIGNATED_INITIALIZER;
- (instancetype)init NS_UNAVAILABLE;
- (BOOL)addSku:(NSString *)sku quantity:(NSInteger)quantity error:(NSError **)error;
- (void)enumerateUsingBlock:(void (^)(NSString *sku, NSInteger quantity, BOOL *stop))block;

@end

NS_ASSUME_NONNULL_END
