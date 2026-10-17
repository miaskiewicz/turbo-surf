#pragma once
#include <cstdlib>
#define CHECK(x) do{if(!(x))std::abort();}while(false)
#define CHECK_EQ(a,b) CHECK((a)==(b))
#define DCHECK(...) ((void)0)
#define DCHECK_EQ(...) ((void)0)
#define DCHECK_GE(...) ((void)0)
#define DCHECK_LT(...) ((void)0)
