package nl.trackerr.p56studio

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.net.wifi.WifiManager
import android.os.Bundle
import android.view.WindowManager
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  private var multicastLock: WifiManager.MulticastLock? = null
  private var wifiCallback: ConnectivityManager.NetworkCallback? = null

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    // A spinning fan show is watched, not touched: keep the screen on while the app is in
    // front (a window flag, so it has no effect once the app is in the background).
    window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
  }

  // The multicast lock and the Wi-Fi binding are only held while the app is visible, so a
  // backgrounded app does not keep the radio busy.
  override fun onStart() {
    super.onStart()
    // Many phones drop incoming UDP broadcasts unless a multicast lock is held.
    if (multicastLock == null) {
      val wifi = applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
      multicastLock = wifi.createMulticastLock("p56").apply { setReferenceCounted(false); acquire() }
    }
    // The fan's own Wi-Fi has no internet, so Android sends traffic over mobile data instead.
    // Ask for a Wi-Fi network (without the internet requirement) and bind the app to it, so the
    // discovery broadcast and the fan's TCP connection use Wi-Fi.
    if (wifiCallback == null) {
      val cm = getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
      val request = NetworkRequest.Builder()
        .addTransportType(NetworkCapabilities.TRANSPORT_WIFI)
        .removeCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
        .build()
      val cb = object : ConnectivityManager.NetworkCallback() {
        override fun onAvailable(network: Network) { cm.bindProcessToNetwork(network) }
        override fun onLost(network: Network) {
          if (cm.boundNetworkForProcess == network) cm.bindProcessToNetwork(null)
        }
      }
      try {
        cm.requestNetwork(request, cb)
        wifiCallback = cb
      } catch (e: Exception) {
        // Without CHANGE_NETWORK_STATE (or on odd ROMs) fall back to the default network.
      }
    }
  }

  override fun onStop() {
    release()
    super.onStop()
  }

  override fun onDestroy() {
    release()
    super.onDestroy()
  }

  private fun release() {
    wifiCallback?.let {
      val cm = getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
      try { cm.unregisterNetworkCallback(it) } catch (_: Exception) {}
      cm.bindProcessToNetwork(null)
    }
    wifiCallback = null
    multicastLock?.let { if (it.isHeld) it.release() }
    multicastLock = null
  }
}
