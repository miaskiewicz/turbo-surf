#pragma once
#include "base/containers/span.h"
namespace surf_audio_effects{
class AudioChannel{float*data_;size_t length_;public:AudioChannel(float*p,size_t n):data_(p),length_(n){}base::span<const float>Span()const{return {data_,length_};}base::span<float>MutableSpan(){return {data_,length_};}};
class AudioBus{unsigned channels_;size_t length_;AudioChannel channels[2];public:enum{kChannelLeft=0,kChannelRight=1};AudioBus(unsigned count,size_t length,float*l,float*r):channels_(count),length_(length),channels{{l,length},{r,length}}{}unsigned NumberOfChannels()const{return channels_;}size_t length()const{return length_;}const AudioChannel*Channel(unsigned i)const{return &channels[i];}AudioChannel*ChannelByType(unsigned i){return &channels[i];}};
}
