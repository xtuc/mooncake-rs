/**
 * Mooncake FFI C API Implementation
 * 
 * Wraps Mooncake C++ TransferEngine for Rust FFI.
 */

#include "mooncake_ffi.h"
#include <mooncake/transfer_engine.h>
#include <cstring>
#include <memory>
#include <string>
#include <unordered_map>
#include <mutex>

// Internal state tracking
struct BufferInfo {
    std::shared_ptr<mooncake::BufferDesc> desc;
    size_t size;
    void* ptr;
};

struct EngineState {
    std::unique_ptr<mooncake::TransferEngine> engine;
    std::unordered_map<std::string, BufferInfo> exposed_buffers;
    std::mutex mutex;
};

extern "C" {

MooncakeEngine* mooncake_engine_create(const char* metadata_server,
                                        const char* local_addr) {
    try {
        auto state = new EngineState();
        state->engine = std::make_unique<mooncake::TransferEngine>(
            metadata_server,
            local_addr
        );
        return reinterpret_cast<MooncakeEngine*>(state);
    } catch (...) {
        return nullptr;
    }
}

void mooncake_engine_destroy(MooncakeEngine* engine) {
    if (!engine) return;
    
    auto* state = reinterpret_cast<EngineState*>(engine);
    
    // Revoke all exposed buffers before destroying
    {
        std::lock_guard<std::mutex> lock(state->mutex);
        for (const auto& [request_id, info] : state->exposed_buffers) {
            state->engine->revokeBuffer(request_id);
        }
    }
    
    delete state;
}

MooncakeBuffer* mooncake_register_memory(MooncakeEngine* engine,
                                          void* ptr,
                                          size_t size) {
    if (!engine || !ptr || size == 0) return nullptr;
    
    try {
        auto* state = reinterpret_cast<EngineState*>(engine);
        
        // Register with Mooncake
        auto desc = state->engine->registerLocalMemory(ptr, size);
        if (!desc) return nullptr;
        
        // Allocate our wrapper
        auto* buffer = new MooncakeBuffer();
        // Store the buffer descriptor pointer in the opaque struct
        // We'll use a hack: store the shared_ptr in a map keyed by buffer address
        static std::unordered_map<MooncakeBuffer*, std::shared_ptr<mooncake::BufferDesc>> buffer_map;
        static std::mutex buffer_mutex;
        
        {
            std::lock_guard<std::mutex> lock(buffer_mutex);
            buffer_map[buffer] = desc;
        }
        
        return buffer;
    } catch (...) {
        return nullptr;
    }
}

int mooncake_expose_buffer(MooncakeEngine* engine,
                           MooncakeBuffer* buffer,
                           const char* request_id) {
    if (!engine || !buffer || !request_id) return -1;
    
    try {
        auto* state = reinterpret_cast<EngineState*>(engine);
        
        // Retrieve the buffer descriptor
        static std::unordered_map<MooncakeBuffer*, std::shared_ptr<mooncake::BufferDesc>> buffer_map;
        static std::mutex buffer_mutex;
        
        std::shared_ptr<mooncake::BufferDesc> desc;
        {
            std::lock_guard<std::mutex> lock(buffer_mutex);
            auto it = buffer_map.find(buffer);
            if (it == buffer_map.end()) return -1;
            desc = it->second;
        }
        
        // Expose via metadata service
        if (!state->engine->exposeBuffer(desc, request_id)) {
            return -1;
        }
        
        // Track in our state
        {
            std::lock_guard<std::mutex> lock(state->mutex);
            state->exposed_buffers[request_id] = BufferInfo{desc, desc->size, desc->addr};
        }
        
        return 0;
    } catch (...) {
        return -1;
    }
}

int mooncake_revoke_buffer(MooncakeEngine* engine,
                           const char* request_id) {
    if (!engine || !request_id) return -1;
    
    try {
        auto* state = reinterpret_cast<EngineState*>(engine);
        
        {
            std::lock_guard<std::mutex> lock(state->mutex);
            auto it = state->exposed_buffers.find(request_id);
            if (it == state->exposed_buffers.end()) return -1;
            
            state->engine->revokeBuffer(request_id);
            state->exposed_buffers.erase(it);
        }
        
        return 0;
    } catch (...) {
        return -1;
    }
}

int mooncake_transfer(MooncakeEngine* engine,
                      const char* request_id,
                      void* dst_ptr,
                      size_t size) {
    if (!engine || !request_id || !dst_ptr || size == 0) return -1;
    
    try {
        auto* state = reinterpret_cast<EngineState*>(engine);
        
        // Initiate transfer
        auto status = state->engine->transfer(request_id, dst_ptr, size);
        
        if (!status.ok()) {
            return -1;
        }
        
        // Wait for completion (synchronous for now)
        // TODO: Make async with callback
        status = state->engine->waitForCompletion(request_id);
        
        return status.ok() ? 0 : -1;
    } catch (...) {
        return -1;
    }
}

} // extern "C"
