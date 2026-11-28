#import <Foundation/Foundation.h>

NS_ASSUME_NONNULL_BEGIN

/// Swift can't catch an Objective-C exception, so tests that check one is
/// raised, rather than a trap, run the code here.
@interface ObjCExceptions : NSObject
+ (nullable NSException *)catching:(NS_NOESCAPE void (^)(void))block;
@end

NS_ASSUME_NONNULL_END
