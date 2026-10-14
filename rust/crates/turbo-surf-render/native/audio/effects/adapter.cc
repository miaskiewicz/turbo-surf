// Standalone adapters over the licensed Chrome155 kernels in this directory.
#include "effects/platform.h"
#include "third_party/blink/renderer/platform/audio/biquad.h"
#include "third_party/blink/renderer/platform/audio/delay.h"
#include "third_party/blink/renderer/platform/audio/stereo_panner.h"
#include "third_party/blink/renderer/platform/audio/audio_bus.h"
#include <algorithm>
#include <array>
#include <cmath>
#include <memory>
#include <limits>
#include <vector>
namespace {
using namespace surf_audio_effects;
struct Effect {
  unsigned kind, shape;
  float sample_rate;
  double current_time=0,last_non_silent_time=0,tail_time=0;
  bool input_silent=false,output_silent=false;
  std::array<std::unique_ptr<Biquad>,2> biquads;
  std::array<std::unique_ptr<Delay>,2> delays;
  std::unique_ptr<StereoPanner> panner;
  std::vector<float> curve;
  Effect(unsigned kind,unsigned shape,float rate,float maximum,const float* curve,size_t length)
      :kind(kind),shape(shape),sample_rate(rate) {
    if(length)this->curve.assign(curve,curve+length);
    if(kind==0)for(auto& b:biquads)b=std::make_unique<Biquad>(128);
    if(kind==1){tail_time=maximum;for(auto& d:delays)d=std::make_unique<Delay>(maximum,rate,128);}
    if(kind==2)panner=std::make_unique<StereoPanner>(rate);
    if(kind==3 && length){
      const double index=.5*(length-1);const size_t first=static_cast<size_t>(index);
      const size_t second=std::min(first+1,length-1);const double fraction=index-first;
      const double at_zero=(1-fraction)*this->curve[first]+fraction*this->curve[second];
      tail_time=at_zero==0?0:std::numeric_limits<double>::infinity();
    }
  }
  void coefficients(Biquad& filter,int index,const std::array<const float*,4>& p) {
    const float detune=p[3][index];
    const double normalized=p[0][index]/(.5*sample_rate);
    const double frequency=detune?normalized*exp2(detune/1200):normalized;
    const double q=p[1][index],gain=p[2][index];
    switch(shape){
      case 0:filter.SetLowpassParams(index,frequency,q);break;
      case 1:filter.SetHighpassParams(index,frequency,q);break;
      case 2:filter.SetBandpassParams(index,frequency,q);break;
      case 3:filter.SetLowShelfParams(index,frequency,gain);break;
      case 4:filter.SetHighShelfParams(index,frequency,gain);break;
      case 5:filter.SetPeakingParams(index,frequency,q,gain);break;
      case 6:filter.SetNotchParams(index,frequency,q);break;
      case 7:filter.SetAllpassParams(index,frequency,q);break;
    }
  }
  void process(float* left,float* right,unsigned channels,const std::array<const float*,4>& p,bool accurate) {
    output_silent=input_silent && last_non_silent_time+tail_time<current_time;
    if(output_silent){std::fill(left,left+128,0.f);std::fill(right,right+128,0.f);return;}
    if(kind==0){
      bool constant=true;
      if(accurate)for(const auto* param:p)for(unsigned i=1;i<128;i++)constant=constant&&param[i]==param[0];
      for(unsigned channel=0;channel<2;channel++){
        auto& filter=*biquads[channel];filter.SetHasSampleAccurateValues(!constant);
        for(unsigned i=0;i<(constant?1u:128u);i++)coefficients(filter,i,p);
        float* data=channel?right:left;filter.Process({data,128},{data,128});
        if(channel==0)tail_time=std::clamp(filter.TailFrame(constant?0:127,30*sample_rate)/sample_rate,0.,30.);
      }
    }else if(kind==1){
      for(unsigned channel=0;channel<2;channel++){
        auto& delay=*delays[channel];float* data=channel?right:left;
        if(accurate){std::copy(p[0],p[0]+128,delay.DelayTimes().begin());delay.ProcessARate({data,128},{data,128},128);}
        else{delay.SetDelayTime(p[0][0]);delay.ProcessKRate({data,128},{data,128},128);}
      }
    }else if(kind==2){
      AudioBus input(channels,128,left,right),output(2,128,left,right);
      if(accurate)panner->PanWithSampleAccurateValues(&input,&output,{p[0],128});
      else panner->PanToTargetValue(&input,&output,p[0][0],128);
    }else if(kind==3 && !curve.empty()){
      // Preserve Blink's separate float add/multiply/interpolation steps;
      // double interpolation and fused operations change the output samples.
      for(float* data:{left,right})for(unsigned i=0;i<128;i++){
        const float shifted=data[i]+1.f;
        const float virtual_index=std::clamp(shifted*static_cast<float>(.5*(curve.size()-1)),0.f,static_cast<float>(curve.size()-1));
        const size_t first=std::isnan(virtual_index)?0:static_cast<size_t>(virtual_index);
        const size_t second=std::min(first+1,curve.size()-1);
        const float fraction=virtual_index-static_cast<float>(first);
        const float difference=curve[second]-curve[first];
        const float product=fraction*difference;
        data[i]=product+curve[first];
      }
    }
    if(!input_silent)last_non_silent_time=current_time+128./sample_rate;
  }
};
}
extern "C" void* surf_audio_effect_new(unsigned kind,unsigned shape,float rate,float maximum,const float* curve,size_t length){
  try{return new Effect(kind,shape,rate,maximum,curve,length);}catch(...){return nullptr;}
}
extern "C" void surf_audio_effect_process(void* pointer,float* left,float* right,unsigned channels,const float* p0,const float* p1,const float* p2,const float* p3,bool accurate){
  static_cast<Effect*>(pointer)->process(left,right,channels,{p0,p1,p2,p3},accurate);
}
extern "C" void surf_audio_effect_free(void* pointer){delete static_cast<Effect*>(pointer);}

extern "C" void surf_audio_effect_set_input(void* pointer,bool silent,double current_time){
 auto* effect=static_cast<Effect*>(pointer);effect->input_silent=silent;effect->current_time=current_time;
}
extern "C" bool surf_audio_effect_output_silent(void* pointer){return static_cast<Effect*>(pointer)->output_silent;}
extern "C" void surf_audio_biquad_frequency_response(void* pointer,const float* parameters,const float* frequency_hz,size_t length,float* magnitude,float* phase){
  auto* effect=static_cast<Effect*>(pointer);
  // Blink uses a separate response kernel, leaving active processing/history alone.
  surf_audio_effects::Biquad response(128);
  const std::array<const float*,4> values={parameters,parameters+1,parameters+2,parameters+3};
  effect->coefficients(response,0,values);
  std::vector<float> frequency(length);
  const double nyquist=.5*effect->sample_rate;
  for(size_t index=0;index<length;index++)frequency[index]=frequency_hz[index]/nyquist;
  response.GetFrequencyResponse({frequency.data(),length},{magnitude,length},{phase,length});
}
