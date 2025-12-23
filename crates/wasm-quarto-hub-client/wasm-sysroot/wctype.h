#pragma once

typedef __WCHAR_TYPE__ wchar_t;
typedef __WINT_TYPE__ wint_t;

int iswspace(wchar_t ch);
int iswalnum(wint_t _wc);
int iswdigit(wint_t c);
int iswalpha(wint_t c);
wint_t towlower(wint_t wc);
