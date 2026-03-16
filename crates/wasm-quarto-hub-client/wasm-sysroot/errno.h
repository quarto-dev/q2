#pragma once

#define EDOM   33
#define ERANGE 34
#define EILSEQ 84

extern int *__errno_location(void);
#define errno (*__errno_location())
