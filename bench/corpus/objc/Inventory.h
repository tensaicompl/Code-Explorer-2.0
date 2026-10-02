#import <Foundation/Foundation.h>

typedef enum CRPStatus : NSInteger {
    CRPStatusPending,
    CRPStatusDone,
} CRPStatus;

@protocol CRPObserver <NSObject>
- (void)inventoryDidChange:(id)inventory;
@optional
- (void)inventoryWillReset:(id)inventory;
@end

@interface CRPInventory : NSObject <NSCopying>

@property (nonatomic, copy, readonly) NSDictionary<NSString *, NSNumber *> *items;
@property (nonatomic, weak, nullable) id<CRPObserver> observer;
@property (nonatomic) CRPStatus status;

- (instancetype)initWithLimit:(NSUInteger)limit;
- (instancetype)init NS_UNAVAILABLE;
- (BOOL)addSku:(NSString *)sku quantity:(NSInteger)quantity error:(NSError **)error;
- (void)enumerateUsingBlock:(void (^)(NSString *sku, NSInteger quantity, BOOL *stop))block;

@end
