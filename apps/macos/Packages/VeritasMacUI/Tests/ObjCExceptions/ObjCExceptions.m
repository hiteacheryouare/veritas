#import "ObjCExceptions.h"

@implementation ObjCExceptions
+ (nullable NSException *)catching:(NS_NOESCAPE void (^)(void))block {
	@try {
		block();
	} @catch (NSException *exception) {
		return exception;
	}
	return nil;
}
@end
