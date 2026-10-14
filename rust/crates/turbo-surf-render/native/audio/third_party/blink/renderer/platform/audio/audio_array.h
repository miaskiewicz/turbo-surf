#pragma once
#include <vector>
#include <span>
#include <algorithm>
namespace blink { class AudioFloatArray{std::vector<float> values_;public:AudioFloatArray()=default;explicit AudioFloatArray(unsigned n):values_(n){}std::span<float> as_span(){return values_;}std::span<const float> as_span()const{return values_;}float&at(unsigned i){return values_.at(i);}unsigned size()const{return values_.size();}float&operator[](unsigned i){return values_[i];}void Zero(){std::fill(values_.begin(),values_.end(),0.);}bool TryAllocate(unsigned n){values_.resize(n);return true;}}; }
