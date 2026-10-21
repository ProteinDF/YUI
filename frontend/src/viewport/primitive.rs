use iced::Rectangle;
use iced::widget::shader::{self, Viewport};

use super::gpu_types::*;
use super::pipeline::{ViewportId, ViewportPipeline};
use crate::culling::Frustum;
use crate::math::vec3_normalize;
use crate::ribbon::RibbonVertex;
use yui_core::state::{CameraState, Color4};

/// 描画プリミティブ
#[derive(Debug)]
pub struct ViewportPrimitive {
    /// どの分子図の資源（バッファ・ユニフォーム）に書き込み、描くか
    pub(crate) id: ViewportId,
    pub(crate) camera: CameraState,
    pub(crate) background_color: Color4,
    pub(crate) sphere_instances: Vec<InstanceRaw>,
    pub(crate) stick_instances: Vec<InstanceRaw>,
    pub(crate) ribbon_vertices: Vec<RibbonVertex>,
    pub(crate) ribbon_indices: Vec<u32>,
    pub(crate) surface_batches: crate::surface_render::SurfaceBatches,
    pub(crate) interaction_instances: Vec<InstanceRaw>,
    /// 選択の強調のインスタンス（空なら強調しない）
    pub(crate) selection_instances: Vec<InstanceRaw>,
    /// 強調の方法（`Off` のときは、`selection_instances` は空）
    pub(crate) highlight: crate::selection::HighlightStyle,
    /// 背景を描くか（`false` なら、描画先の下地のまま。背景を透明にした PNG の書き出しで使う）
    pub(crate) draw_background: bool,
    /// SSAO の強さ（0 でオフ。オフのときは、SSAO のパスを動かさない）と、遮蔽を探す半径（Å）
    pub(crate) ssao_strength: f32,
    pub(crate) ssao_radius: f32,
}

impl shader::Primitive for ViewportPrimitive {
    type Pipeline = ViewportPipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &Viewport,
    ) {
        let width = viewport.physical_width().max(1);
        let height = viewport.physical_height().max(1);

        // 深度テクスチャのサイズはカラーターゲット（ウィンドウ物理サーフェス）と一致させる
        pipeline.update_depth_texture(device, width, height);

        // この分子図の資源（バッファ・ユニフォーム）を用意する。他の分子図の資源には触れない。
        pipeline.ensure_viewport(self.id, device);
        let counts = pipeline.mesh_counts();
        let cull_layout = &pipeline.cull_bind_group_layout;
        let res = pipeline
            .viewports
            .get_mut(&self.id)
            .expect("ensure_viewport created the resources");

        // 背景色の更新（ユニフォームバッファへの書き込み）
        let bg_uniforms = BackgroundUniforms {
            color: self.background_color,
        };
        queue.write_buffer(
            &res.background_uniform_buffer,
            0,
            bytemuck::bytes_of(&bg_uniforms),
        );

        // カメラ行列の計算（ペインのバウンズのアスペクト比を使用）
        let aspect = bounds.width / bounds.height.max(1.0);
        let view_proj = super::camera::view_proj_matrix(&self.camera, aspect);

        let f = vec3_normalize(crate::math::vec3_sub(self.camera.target, self.camera.eye));
        let s = vec3_normalize(crate::math::vec3_cross(f, self.camera.up));
        let u = crate::math::vec3_cross(s, f);

        let uniforms = CameraUniforms {
            view_proj,
            light_dir: [0.5, 0.8, 0.6, 0.0],
            camera_right: [s[0], s[1], s[2], 0.0],
            camera_up: [u[0], u[1], u[2], 0.0],
            style: {
                let r = crate::user_settings::startup_render();
                [r.interaction_dash, r.interaction_gap, r.selection_size, 0.0]
            },
        };
        queue.write_buffer(&res.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        // SSAO（強さが 0 のときは、何もしない）。深度テクスチャは、リサイズで作り直すので、毎回束ね直す
        res.ssao_bind_group = None;
        res.ssao_blur_bind_group = None;
        if self.ssao_strength > 0.0
            && let Some(depth_view) = pipeline.depth_texture_view.as_ref()
            && let Some(ao_view) = pipeline.ao_texture_view.as_ref()
        {
            let scale = viewport.scale_factor();
            let tan_half_y = (self.camera.fov.to_radians() * 0.5).tan();
            let ssao = SsaoUniforms {
                near: super::camera::NEAR_PLANE,
                far: super::camera::far_plane_for(&self.camera),
                tan_half_y,
                tan_half_x: tan_half_y * aspect,
                pane: [
                    bounds.x * scale,
                    bounds.y * scale,
                    bounds.width * scale,
                    bounds.height * scale,
                ],
                radius: self.ssao_radius,
                strength: self.ssao_strength.min(1.0),
                _pad: [0.0; 2],
            };
            queue.write_buffer(&res.ssao_uniform_buffer, 0, bytemuck::bytes_of(&ssao));
            res.ssao_bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("SSAO Bind Group"),
                layout: &pipeline.ssao_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(depth_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: res.ssao_uniform_buffer.as_entire_binding(),
                    },
                ],
            }));
            res.ssao_blur_bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("SSAO Blur Bind Group"),
                layout: &pipeline.ssao_blur_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(depth_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: res.ssao_uniform_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(ao_view),
                    },
                ],
            }));
        }

        // 視錐台の抽出とインスタンス・カリング用バッファの更新
        let frustum = Frustum::from_view_proj(&view_proj);
        res.update_instances(
            device,
            queue,
            cull_layout,
            counts,
            &self.sphere_instances,
            &self.stick_instances,
            &frustum,
            self.camera.eye,
        );

        // カリング & 段階的LOD効果の実測・検証ログ出力
        //
        // NOTE: バウンディング半径は cs_cull（WGSL）と同じ「各列ベクトルの全成分ノルムの和」で
        // 算出する。対角成分のみ（例: model_0[0]^2）を使う簡略式は、回転を含む変換行列
        // （from_cylinder によるスティック等）では過小評価となり、GPU側の判定と食い違うため使わない。
        fn instance_bounding_radius(inst: &InstanceRaw) -> f32 {
            let col_norm_sq = |c: [f32; 4]| c[0] * c[0] + c[1] * c[1] + c[2] * c[2];
            (col_norm_sq(inst.model_0) + col_norm_sq(inst.model_1) + col_norm_sq(inst.model_2))
                .sqrt()
        }

        if !self.sphere_instances.is_empty() || !self.stick_instances.is_empty() {
            let threshold_sq =
                crate::lod::DEFAULT_LOD_MID_DISTANCE * crate::lod::DEFAULT_LOD_MID_DISTANCE;
            let mut near_spheres = 0;
            let mut mid_spheres = 0;
            let mut vis_spheres = 0;

            for inst in &self.sphere_instances {
                let c = [inst.model_3[0], inst.model_3[1], inst.model_3[2]];
                if frustum.intersects_sphere(c, instance_bounding_radius(inst)) {
                    vis_spheres += 1;
                    let diff = [
                        c[0] - self.camera.eye[0],
                        c[1] - self.camera.eye[1],
                        c[2] - self.camera.eye[2],
                    ];
                    let dist_sq = diff[0] * diff[0] + diff[1] * diff[1] + diff[2] * diff[2];
                    if dist_sq <= threshold_sq {
                        near_spheres += 1;
                    } else {
                        mid_spheres += 1;
                    }
                }
            }

            let vis_sticks = self
                .stick_instances
                .iter()
                .filter(|inst| {
                    let c = [inst.model_3[0], inst.model_3[1], inst.model_3[2]];
                    if frustum.intersects_sphere(c, instance_bounding_radius(inst)) {
                        let diff = [
                            c[0] - self.camera.eye[0],
                            c[1] - self.camera.eye[1],
                            c[2] - self.camera.eye[2],
                        ];
                        let dist_sq = diff[0] * diff[0] + diff[1] * diff[1] + diff[2] * diff[2];
                        dist_sq <= threshold_sq
                    } else {
                        false
                    }
                })
                .count();

            println!(
                "[Culling & LOD] Spheres visible: {}/{} (Near mesh: {}, Mid impostor: {}), Sticks visible: {}/{}",
                vis_spheres,
                self.sphere_instances.len(),
                near_spheres,
                mid_spheres,
                vis_sticks,
                self.stick_instances.len()
            );
        }

        // リボンバッファの更新
        res.update_ribbon_buffers(device, queue, &self.ribbon_vertices, &self.ribbon_indices);

        // 分子表面バッファの更新
        res.update_surface_buffers(device, queue, &self.surface_batches);

        // 相互作用バッファの更新
        res.update_interaction_buffers(device, queue, &self.interaction_instances);

        // 選択の強調バッファの更新
        res.update_selection_buffers(device, queue, &self.selection_instances);
    }

    fn render(
        &self,
        pipeline: &Self::Pipeline,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip_bounds: &Rectangle<u32>,
    ) {
        let Some(ref depth_view) = pipeline.depth_texture_view else {
            return;
        };
        let Some(res) = pipeline.viewports.get(&self.id) else {
            return;
        };

        if clip_bounds.width == 0 || clip_bounds.height == 0 {
            return;
        }

        // 1. Compute Pass: 視錐台カリング & インスタンス・コンパクション
        {
            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Frustum Culling ComputePass"),
                timestamp_writes: None,
            });
            compute_pass.set_pipeline(&pipeline.cull_pipeline);

            // 球（原子）のカリング
            if res.sphere_total_count > 0
                && let Some(ref bg) = res.sphere_cull_bind_group
            {
                compute_pass.set_bind_group(0, bg, &[]);
                let workgroups = res.sphere_total_count.div_ceil(64);
                compute_pass.dispatch_workgroups(workgroups, 1, 1);
            }

            // スティック（結合）のカリング
            if res.stick_total_count > 0
                && let Some(ref bg) = res.stick_cull_bind_group
            {
                compute_pass.set_bind_group(0, bg, &[]);
                let workgroups = res.stick_total_count.div_ceil(64);
                compute_pass.dispatch_workgroups(workgroups, 1, 1);
            }
        }

        // 2. Render Pass: 3D描画
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Molecule 3D RenderPass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        // 3D描画領域をペインのクリップ境界に合わせる
        pass.set_viewport(
            clip_bounds.x as f32,
            clip_bounds.y as f32,
            clip_bounds.width as f32,
            clip_bounds.height as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(
            clip_bounds.x,
            clip_bounds.y,
            clip_bounds.width,
            clip_bounds.height,
        );

        // 0. 背景の描画（scissor rect で制限されたビューポートペイン内のみを背景色で塗りつぶす）
        if self.draw_background {
            pass.set_pipeline(&pipeline.background_pipeline);
            pass.set_bind_group(0, &res.background_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        // 1. カートゥーン（リボン）の描画
        if res.ribbon_index_count > 0 {
            pass.set_pipeline(&pipeline.ribbon_render_pipeline);
            pass.set_bind_group(0, &res.bind_group, &[]);
            pass.set_vertex_buffer(0, res.ribbon_vertex_buffer.slice(..));
            pass.set_index_buffer(res.ribbon_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..res.ribbon_index_count, 0, 0..1);
        }

        // 2. 相互作用（破線円柱）の直接描画（原子数よりはるかに少ないためGPUカリングを通さずに直接描画）
        if res.interaction_total_count > 0 {
            pass.set_pipeline(&pipeline.interaction_render_pipeline);
            pass.set_bind_group(0, &res.bind_group, &[]);
            pass.set_vertex_buffer(0, pipeline.cylinder_vertex_buffer.slice(..));
            pass.set_vertex_buffer(1, res.interaction_instance_buffer.slice(..));
            pass.set_index_buffer(
                pipeline.cylinder_index_buffer.slice(..),
                wgpu::IndexFormat::Uint16,
            );
            pass.draw_indexed(
                0..pipeline.cylinder_index_count,
                0,
                0..res.interaction_total_count,
            );
        }

        pass.set_pipeline(&pipeline.render_pipeline);
        pass.set_bind_group(0, &res.bind_group, &[]);

        // 3. スティック（結合）の描画（draw_indexed_indirect: 近距離のみ）
        if res.stick_total_count > 0 {
            pass.set_pipeline(&pipeline.render_pipeline);
            pass.set_bind_group(0, &res.bind_group, &[]);
            pass.set_vertex_buffer(0, pipeline.cylinder_vertex_buffer.slice(..));
            pass.set_vertex_buffer(1, res.stick_dst_buffer.slice(..));
            pass.set_index_buffer(
                pipeline.cylinder_index_buffer.slice(..),
                wgpu::IndexFormat::Uint16,
            );
            pass.draw_indexed_indirect(&res.stick_indirect_buffer, 0);
        }

        // 4. 球（原子）の描画（段階的LOD: 近距離メッシュ球 + 中距離インポスター球）
        if res.sphere_total_count > 0 {
            // 近距離 (Near Tier): フルジオメトリ球
            pass.set_pipeline(&pipeline.render_pipeline);
            pass.set_bind_group(0, &res.bind_group, &[]);
            pass.set_vertex_buffer(0, pipeline.sphere_vertex_buffer.slice(..));
            pass.set_vertex_buffer(1, res.sphere_dst_buffer_near.slice(..));
            pass.set_index_buffer(
                pipeline.sphere_index_buffer.slice(..),
                wgpu::IndexFormat::Uint16,
            );
            pass.draw_indexed_indirect(&res.sphere_indirect_buffer_near, 0);

            // 中距離 (Mid Tier): ビルボードインポスター球
            pass.set_pipeline(&pipeline.impostor_render_pipeline);
            pass.set_bind_group(0, &res.bind_group, &[]);
            pass.set_vertex_buffer(0, pipeline.impostor_quad_vertex_buffer.slice(..));
            pass.set_vertex_buffer(1, res.sphere_dst_buffer_mid.slice(..));
            pass.set_index_buffer(
                pipeline.impostor_quad_index_buffer.slice(..),
                wgpu::IndexFormat::Uint16,
            );
            pass.draw_indexed_indirect(&res.sphere_indirect_buffer_mid, 0);
        }

        // 5. 不透明分子表面の描画
        if res.surface_opaque_index_count > 0 {
            pass.set_pipeline(&pipeline.ribbon_render_pipeline);
            pass.set_bind_group(0, &res.bind_group, &[]);
            pass.set_vertex_buffer(0, res.surface_opaque_vertex_buffer.slice(..));
            pass.set_index_buffer(
                res.surface_opaque_index_buffer.slice(..),
                wgpu::IndexFormat::Uint32,
            );
            pass.draw_indexed(0..res.surface_opaque_index_count, 0, 0..1);
        }

        // 6. 半透明分子表面の描画（不透明描画後、深度書き込み無効、アルファブレンド）
        if res.surface_transparent_index_count > 0 {
            pass.set_pipeline(&pipeline.surface_transparent_render_pipeline);
            pass.set_bind_group(0, &res.bind_group, &[]);
            pass.set_vertex_buffer(0, res.surface_transparent_vertex_buffer.slice(..));
            pass.set_index_buffer(
                res.surface_transparent_index_buffer.slice(..),
                wgpu::IndexFormat::Uint32,
            );
            pass.draw_indexed(0..res.surface_transparent_index_count, 0, 0..1);
        }

        // 7. 選択の強調（ハロー / グロー）。シーンの深度で深度テストし、深度は書かない。
        if res.selection_total_count > 0 {
            use crate::selection::HighlightStyle;
            let highlight_pipeline = match self.highlight {
                HighlightStyle::Halo => Some(&pipeline.selection_halo_pipeline),
                HighlightStyle::Glow => Some(&pipeline.selection_glow_pipeline),
                HighlightStyle::Off => None,
            };
            if let Some(highlight_pipeline) = highlight_pipeline {
                pass.set_pipeline(highlight_pipeline);
                pass.set_bind_group(0, &res.bind_group, &[]);
                pass.set_vertex_buffer(0, pipeline.impostor_quad_vertex_buffer.slice(..));
                pass.set_vertex_buffer(1, res.selection_instance_buffer.slice(..));
                pass.set_index_buffer(
                    pipeline.impostor_quad_index_buffer.slice(..),
                    wgpu::IndexFormat::Uint16,
                );
                pass.draw_indexed(
                    0..pipeline.impostor_quad_index_count,
                    0,
                    0..res.selection_total_count,
                );
            }
        }
        drop(pass);

        // 8. SSAO（2 段）。深度を読むので、深度を書いた上のパスとは別のパスにする。
        // 8a. 描いたあとの深度から、遮蔽の係数を求めて、専用のテクスチャに描く（粒が残る）。
        // 8b. 深度が近い隣とだけ混ぜて（ぼかし）、今の色に掛ける（隅や隙間が暗くなる）。
        if let (Some(ssao_bind_group), Some(blur_bind_group), Some(ao_view)) = (
            res.ssao_bind_group.as_ref(),
            res.ssao_blur_bind_group.as_ref(),
            pipeline.ao_texture_view.as_ref(),
        ) {
            let set_pane = |pass: &mut wgpu::RenderPass<'_>| {
                pass.set_viewport(
                    clip_bounds.x as f32,
                    clip_bounds.y as f32,
                    clip_bounds.width as f32,
                    clip_bounds.height as f32,
                    0.0,
                    1.0,
                );
                pass.set_scissor_rect(
                    clip_bounds.x,
                    clip_bounds.y,
                    clip_bounds.width,
                    clip_bounds.height,
                );
            };
            let mut ao_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("SSAO RenderPass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: ao_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        // 描かないところ（分子図の外）は 1（遮蔽なし）
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            set_pane(&mut ao_pass);
            ao_pass.set_pipeline(&pipeline.ssao_pipeline);
            ao_pass.set_bind_group(0, ssao_bind_group, &[]);
            ao_pass.draw(0..3, 0..1);
            drop(ao_pass);

            let mut blur_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("SSAO Blur RenderPass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            set_pane(&mut blur_pass);
            blur_pass.set_pipeline(&pipeline.ssao_blur_pipeline);
            blur_pass.set_bind_group(0, blur_bind_group, &[]);
            blur_pass.draw(0..3, 0..1);
        }
    }
}
