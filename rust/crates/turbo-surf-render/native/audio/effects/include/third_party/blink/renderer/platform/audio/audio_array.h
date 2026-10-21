#pragma once
#include <vector>
#include <algorithm>
#include "base/containers/span.h"
namespace surf_audio_effects {
template<class T>class Array{std::vector<T> values_;public:Array()=default;explicit Array(size_t n):values_(n){}void Allocate(size_t n){values_.assign(n,T{});}void Zero(){std::fill(values_.begin(),values_.end(),T{});}size_t size()const{return values_.size();}T&operator[](size_t i){return values_[i];}const T&operator[](size_t i)const{return values_[i];}base::span<T>as_span(){return {values_.data(),values_.size()};}base::span<const T>as_span()const{return {values_.data(),values_.size()};}};
using AudioFloatArray=Array<float>;using AudioDoubleArray=Array<double>;}
