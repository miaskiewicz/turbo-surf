#pragma once
#include <vector>
#include <span>
namespace blink{class AudioChannel{std::vector<float> v_;public:explicit AudioChannel(unsigned n):v_(n){}std::span<const float>Span()const{return v_;}std::span<float>MutableSpan(){return v_;}};class AudioBus{std::vector<AudioChannel> ch_;public:AudioBus(unsigned n,unsigned len){for(unsigned i=0;i<n;i++)ch_.emplace_back(len);}unsigned NumberOfChannels()const{return ch_.size();}const AudioChannel*Channel(unsigned i)const{return &ch_[i];}AudioChannel*Channel(unsigned i){return &ch_[i];}};}
