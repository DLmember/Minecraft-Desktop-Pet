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
// Exact, case-insensitive executable basenames. No browser or general video
// player matches: an active session and audible peak are still required.
static bool is_music_player(const wchar_t* name) {
 static const wchar_t* const players[]={
  L"cloudmusic.exe",
  L"qqmusic.exe",
  L"spotify.exe",
  L"kugou.exe",
  L"kuwomusic.exe",
  L"kwmusic.exe",
  L"migumusic.exe",
  L"sodamusic.exe",
  L"ttplayer.exe",
  L"baidumusic.exe",
  L"qianqian.exe",
  L"xiami.exe",
  L"kkbox.exe",
  L"joox.exe",
  L"listen1.exe",
  L"listen 1.exe",
  L"applemusic.exe",
  L"itunes.exe",
  L"amazon music.exe",
  L"amazonmusic.exe",
  L"deezer.exe",
  L"tidal.exe",
  L"qobuz.exe",
  L"pandora.exe",
  L"napster.exe",
  L"youtube music.exe",
  L"youtubemusic.exe",
  L"yandexmusic.exe",
  L"yandex music.exe",
  L"plexamp.exe",
  L"roon.exe",
  L"foobar2000.exe",
  L"aimp.exe",
  L"aimp3.exe",
  L"musicbee.exe",
  L"winamp.exe",
  L"mediamonkey.exe",
  L"clementine.exe",
  L"strawberry.exe",
  L"dopamine.exe",
  L"audacious.exe",
  L"fooyin.exe",
  L"resonic.exe",
  L"resonicpro.exe",
  L"audirvana.exe",
  L"xmplay.exe",
  L"winyl.exe",
  L"songbird.exe",
  L"tauon.exe",
  L"quodlibet.exe",
 };
 for(const wchar_t* player:players)if(_wcsicmp(name,player)==0)return true;
 return false;
}
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
     const bool music=is_music_player(name);
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
