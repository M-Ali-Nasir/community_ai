package ai.community.worker

/**
 * Android JNI / Native Bridge for Community AI Core Engine.
 * Loads libcommunity_ffi.so compiled via cargo ndk.
 */
object CommunityAINative {
    init {
        System.loadLibrary("community_ffi")
    }

    external fun generateIdentity(): String
    external fun tickGovernor(isBusy: Boolean, onBattery: Boolean): String
    external fun planPipeline(modelId: String, modelSizeMb: Long, totalLayers: Int, nodesJson: String): String
    external fun modelManifest(modelId: String, totalLayers: Int, numShards: Int): String
}
