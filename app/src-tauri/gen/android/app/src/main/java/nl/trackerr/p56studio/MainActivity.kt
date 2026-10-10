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
    // A spinning fan show is watched, not touched: keep the screen on while the app is open.
    window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)

    // Many phones drop incoming UDP broadcasts unless a multicast lock is held.
    val wifi = applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
    multicastLock = wifi.createMulticastLock("p56").apply { setReferenceCounted(false); acquire() }

    // The fan's own Wi-Fi has no internet, so Android sends traffic over mobile data instead.
    // Keep a Wi-Fi network (without the internet requirement) and bind the app to it, so the
    // discovery broadcast and the fan's TCP connection use Wi-Fi.
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

  override fun onDestroy() {
    wifiCallback?.let {
      val cm = getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
      try { cm.unregisterNetworkCallback(it) } catch (_: Exception) {}
      cm.bindProcessToNetwork(null)
    }
    multicastLock?.let { if (it.isHeld) it.release() }
    super.onDestroy()
  }
}
