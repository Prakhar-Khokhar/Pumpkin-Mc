use pumpkin_data::{BlockState, BlockStateId};
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::generation::structure::template::BlockPlacer;
use pumpkin_world::generation::structure::template::processor::HeightmapType;
use pumpkin_world::level::Level;

use crate::world::World;

impl World {
    pub fn clear_synced_block_events_in_box(&self, min: &BlockPos, max: &BlockPos) {
        let mut events = self
            .synced_block_event_queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        events.retain(|event| {
            let pos = event.pos;
            pos.0.x < min.0.x
                || pos.0.x >= max.0.x
                || pos.0.y < min.0.y
                || pos.0.y >= max.0.y
                || pos.0.z < min.0.z
                || pos.0.z >= max.0.z
        });
    }
}

pub struct WorldBlockPlacer<'a> {
    world: &'a World,
    pub block_entity_nbts: Vec<NbtCompound>,
    pub changed_positions: Vec<(BlockPos, BlockStateId)>,
}

impl<'a> WorldBlockPlacer<'a> {
    #[must_use]
    pub const fn new(world: &'a World) -> Self {
        Self {
            world,
            block_entity_nbts: Vec::new(),
            changed_positions: Vec::new(),
        }
    }

    #[allow(clippy::unused_async)]
    pub fn finalize(&self) {
        for nbt in &self.block_entity_nbts {
            if let Some(block_entity) = crate::block::entities::block_entity_from_nbt(nbt) {
                self.world.add_block_entity(block_entity);
            }
        }
    }
}

impl BlockPlacer for WorldBlockPlacer<'_> {
    fn get_block_state(&self, pos: &Vector3<i32>) -> BlockStateId {
        self.world
            .get_block_state_id(&BlockPos::new(pos.x, pos.y, pos.z))
    }

    fn set_block_state(&mut self, pos: &Vector3<i32>, state: &BlockState) {
        let block_pos = BlockPos::new(pos.x, pos.y, pos.z);
        Level::set_block_state(&self.world.level, &block_pos, state.id);
        self.changed_positions.push((block_pos, state.id));
    }

    fn add_block_entity(&mut self, nbt: NbtCompound) {
        self.block_entity_nbts.push(nbt);
    }

    fn column_height(&self, heightmap: HeightmapType, x: i32, z: i32) -> i32 {
        use pumpkin_world::chunk::ChunkHeightmapType;

        let chunk_x = x >> 4;
        let chunk_z = z >> 4;

        if let Some(chunk) = self.world.level.get_chunk(chunk_x, chunk_z) {
            let chunk_guard = chunk.read();
            let heightmap_lock = chunk_guard.heightmap.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let min_y = chunk_guard.section.min_y;

            let chunk_hm_type = match heightmap {
                HeightmapType::WorldSurfaceWg | HeightmapType::WorldSurface => ChunkHeightmapType::WorldSurfaceWg,
                HeightmapType::OceanFloorWg | HeightmapType::OceanFloor => ChunkHeightmapType::OceanFloorWg,
                HeightmapType::MotionBlocking => ChunkHeightmapType::MotionBlocking,
                HeightmapType::MotionBlockingNoLeaves => ChunkHeightmapType::MotionBlockingNoLeaves,
            };

            if let Some(height) = heightmap_lock.get(chunk_hm_type, x, z, min_y).into() {
                return height + 1;
            }
        }

        // Fallback: scan from top
        let bottom = self.world.level.dimension().min_y;
        let ceiling = self.world.level.dimension().min_y + self.world.level.dimension().height as i32;

        (bottom..ceiling).rev().find(|&y| {
            let id = self.get_block_state(&Vector3::new(x, y, z));
            let state = BlockState::from_id(id);
            match heightmap {
                HeightmapType::WorldSurfaceWg | HeightmapType::WorldSurface => !state.is_air(),
                HeightmapType::OceanFloorWg | HeightmapType::OceanFloor => {
                    pumpkin_data::fluid::blocks_movement(state, id.to_block_id())
                }
                HeightmapType::MotionBlocking => {
                    pumpkin_data::fluid::blocks_movement(state, id.to_block_id()) || state.is_liquid()
                }
                HeightmapType::MotionBlockingNoLeaves => {
                    let block = id.to_block_id();
                    (pumpkin_data::fluid::blocks_movement(state, block) || state.is_liquid())
                        && !block.has_tag(pumpkin_data::tag::Block::MINECRAFT_LEAVES)
                }
            }
        }).unwrap_or(bottom) + 1
    }
}
