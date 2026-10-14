#pragma once
#include <span>
#include <algorithm>
namespace surf_audio_effects {
template<class T,size_t E=std::dynamic_extent> class Span:public std::span<T,E>{
 public:using std::span<T,E>::span;
 template<class U,size_t F> Span(Span<U,F> value):std::span<T,E>(value.data(),value.size()){}
 Span<T> first(size_t n)const{return {this->data(),n};}
 Span<T> subspan(size_t offset,size_t count=std::dynamic_extent)const{return {this->data()+offset,count==std::dynamic_extent?this->size()-offset:count};}
 Span<T> take_first(size_t n){auto result=first(n);*this=subspan(n);return result;}
 template<class U,size_t F> void copy_from(Span<U,F> source)const{std::copy(source.begin(),source.end(),this->begin());}
};}
namespace base {template<class T,size_t E=std::dynamic_extent>using span=surf_audio_effects::Span<T,E>;}
