#include <windows.h>
#include <mmdeviceapi.h>
#include <audiopolicy.h>
#include <endpointvolume.h>
#include <wchar.h>
// mingw's endpointvolume.h only forward-declares this documented COM interface.
struct PetAudioMeter : IUnknown {
 virtual HRESULT STDMETHODCALLTYPE GetPeakValue(float*)=0;
 virtual HRESULT STDMETHODCALLTYPE GetMeteringChannelCount(UINT*)=0;
 virtual HRESULT STDMETHODCALLTYPE GetChannelsPeakValues(UINT,float*)=0;
 virtual HRESULT STDMETHODCALLTYPE QueryHardwareSupport(DWORD*)=0;
};
static const IID IID_PetAudioMeter={0xc02216f6,0x8c67,0x4b5b,{0x9d,0x00,0xd0,0x08,0xe7,0x3e,0x00,0x64}};
// Called only on the dedicated MTA worker. No recording, decoding or process injection.
extern "C" bool pet_music_poll() {
 IMMDeviceEnumerator* devices=nullptr; IMMDevice* device=nullptr;
 IAudioSessionManager2* manager=nullptr; IAudioSessionEnumerator* sessions=nullptr;
 bool playing=false;
 HRESULT hr=CoCreateInstance(__uuidof(MMDeviceEnumerator),nullptr,CLSCTX_INPROC_SERVER,__uuidof(IMMDeviceEnumerator),(void**)&devices);
 if(SUCCEEDED(hr))hr=devices->GetDefaultAudioEndpoint(eRender,eMultimedia,&device);
 if(SUCCEEDED(hr))hr=device->Activate(__uuidof(IAudioSessionManager2),CLSCTX_INPROC_SERVER,nullptr,(void**)&manager);
 if(SUCCEEDED(hr))hr=manager->GetSessionEnumerator(&sessions);
 int count=0;if(SUCCEEDED(hr))hr=sessions->GetCount(&count);
 for(int i=0;SUCCEEDED(hr)&&i<count&&i<128;i++) {
  IAudioSessionControl* session=nullptr;IAudioSessionControl2* control=nullptr;PetAudioMeter* meter=nullptr;
  if(FAILED(sessions->GetSession(i,&session)))continue;
  AudioSessionState state=AudioSessionStateInactive;
  session->GetState(&state);
  if(state==AudioSessionStateActive && SUCCEEDED(session->QueryInterface(__uuidof(IAudioSessionControl2),(void**)&control))) {
   DWORD pid=0;control->GetProcessId(&pid);
   HANDLE process=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,FALSE,pid);
   wchar_t path[1024]={};DWORD length=1024;
   if(process) {
    if(QueryFullProcessImageNameW(process,0,path,&length)) {
     const wchar_t* name=wcsrchr(path,L'\\');name=name?name+1:path;
     const bool music=_wcsicmp(name,L"cloudmusic.exe")==0 || _wcsicmp(name,L"qqmusic.exe")==0 || _wcsicmp(name,L"spotify.exe")==0;
     if(music && SUCCEEDED(session->QueryInterface(IID_PetAudioMeter,(void**)&meter))) {
      float peak=0; if(SUCCEEDED(meter->GetPeakValue(&peak))&&peak>0.0001f)playing=true;
     }
    }
    CloseHandle(process);
   }
  }
  if(meter)meter->Release();if(control)control->Release();session->Release();
 }
 if(sessions)sessions->Release();if(manager)manager->Release();if(device)device->Release();if(devices)devices->Release();
 return playing;
}
extern "C" bool pet_music_init(){return SUCCEEDED(CoInitializeEx(nullptr,COINIT_MULTITHREADED));}
extern "C" void pet_music_close(){CoUninitialize();}
