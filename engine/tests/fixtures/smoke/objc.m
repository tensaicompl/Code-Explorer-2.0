#import <Foundation/Foundation.h>

@interface Greeter : NSObject
- (NSString *)greet:(NSString *)name;
@end

@implementation Greeter
- (NSString *)greet:(NSString *)name {
    return [@"hi " stringByAppendingString:name];
}
@end
