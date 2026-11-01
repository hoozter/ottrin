$ErrorActionPreference='Stop'
try {
$root='C:\OttrinJobFixtureQA'
$phase=(Get-Content "$root\phase" -Raw).Trim()
$identity=[Security.Principal.WindowsIdentity]::GetCurrent()
$owner=Get-Content "$root\INTERACTIVE_OWNER.json" -Raw|ConvertFrom-Json
if($identity.User.Value-ne$owner.sid){throw 'Wrong actor'}
$admin=([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if(($phase-eq'limited'-and$admin)-or($phase-eq'elevated'-and-not$admin)){throw 'Incorrect token phase'}
if($admin){throw 'Interactive test requires ordinary token'}
$homeRoot=Join-Path $root "data\$phase"
New-Item -ItemType Directory $homeRoot|Out-Null
foreach($pair in @(@('TEMP','t'),@('TMP','t'),@('APPDATA','a'),@('LOCALAPPDATA','l'),@('USERPROFILE','h'),@('HOME','h'))){$path=Join-Path $homeRoot $pair[1];New-Item -ItemType Directory $path -Force|Out-Null;[Environment]::SetEnvironmentVariable($pair[0],$path,'Process')}
$p=$null
try {
 Add-Type -TypeDefinition @'
using System; using System.Text; using System.Runtime.InteropServices; using System.ComponentModel;
public static class TempAlias {
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern uint GetShortPathNameW(string path,StringBuilder output,uint size);
 public static string Short(string path) {var output=new StringBuilder(32768);uint size=GetShortPathNameW(path,output,(uint)output.Capacity);if(size==0||size>=output.Capacity)throw new Win32Exception(Marshal.GetLastWin32Error());return output.ToString();}
}
'@
 $original=$env:TEMP;$short=[TempAlias]::Short($original)
 @{original=$original;short=$short;scope='Only this owned actor and its fixture children use the existing NTFS short alias; no volume or system policy changed'}|ConvertTo-Json|Set-Content "$root\results\$phase-alias.json"
 if($short-ceq$original-or-not$short.Contains('~')){throw 'Existing NTFS short alias unavailable; no reproduction accepted'}
 $env:TEMP=$short;$env:TMP=$short
 $actorTempEntries=@(Get-ChildItem $env:TEMP -Force|ForEach-Object Name)
 $p=Start-Process "$root\fixture-runtime.exe" -ArgumentList @('--nocapture','--test-threads=1') -WorkingDirectory $homeRoot -PassThru -RedirectStandardOutput "$root\results\$phase.stdout" -RedirectStandardError "$root\results\$phase.stderr"
 $null=$p.Handle
 if(-not$p.WaitForExit(180000)){throw 'Native fixture timeout'}
 @{phase=$phase;exit=$p.ExitCode;user=$identity.User.Value;owner=$identity.Owner.Value;administrator=$admin;tempEntries=@(Get-ChildItem $env:TEMP -Force).Count;actorTempEntries=$actorTempEntries;uiRemainingEntries=@(Get-ChildItem $env:TEMP -Force|Where-Object {$_.Name -notin $actorTempEntries}|Select-Object Name,Attributes,Length);remainingTempEntries=@(Get-ChildItem $env:TEMP -Force|Select-Object -First 20 Name,Attributes,Length)}|ConvertTo-Json|Set-Content "$root\results\$phase.json"
 if($p.ExitCode-ne0){throw "UI suite failed $($p.ExitCode)"}
 $out=Get-Content "$root\results\$phase.stdout" -Raw
 foreach($name in @('windows_move_history_reconciles_exact_inverse_without_overriding_its_worker','windows_move_recovery_requires_captured_paths_and_real_drive_bindings','windows_move_undo_admission_tracks_renamed_parent_paths')){if(-not$out.Contains("test tests::$name ... ok")){throw "Required UI case did not pass: $name"}}
 if(-not$out.Contains('test tests::miller_pointer_file_selection_reveals_the_complete_preview ... ok')){throw 'Required Miller case did not pass: miller_pointer_file_selection_reveals_the_complete_preview'}
 if(-not$out.Contains('test tests::entering_miller_reveals_the_current_directory_and_selection_once ... ok')){throw 'Required Miller case did not pass: entering_miller_reveals_the_current_directory_and_selection_once'}
 if(-not$out.Contains('test tests::miller_metadata_values_fit_their_narrow_pane ... ok')){throw 'Required Miller case did not pass: miller_metadata_values_fit_their_narrow_pane'}
 if(-not$out.Contains('test tests::long_breadcrumbs_preserve_toolbar_controls_and_current_folder_visibility ... ok')){throw 'Required Miller case did not pass: long_breadcrumbs_preserve_toolbar_controls_and_current_folder_visibility'}
 if(-not$out.Contains('test tests::miller_details_toggle_and_final_field_remain_accessible_at_large_text ... ok')){throw 'Required Miller case did not pass: miller_details_toggle_and_final_field_remain_accessible_at_large_text'}
 if(-not$out.Contains('test picker::tests::indexed_search_cancels_abandoned_queries_and_discards_their_results ... ok')){throw 'Required picker case did not pass: indexed_search_cancels_abandoned_queries_and_discards_their_results'}
 if(-not$out.Contains('test picker::tests::indexed_save_uses_the_selected_results_parent_and_rechecks_overwrite ... ok')){throw 'Required picker case did not pass: indexed_save_uses_the_selected_results_parent_and_rechecks_overwrite'}
 if(-not$out.Contains('test picker::tests::indexed_unavailability_keeps_actions_visible_and_direct_browsing_recovers ... ok')){throw 'Required picker case did not pass: indexed_unavailability_keeps_actions_visible_and_direct_browsing_recovers'}
 if(-not$out.Contains('test picker::tests::empty_indexed_search_does_not_connect_and_closing_cancels_inflight_work ... ok')){throw 'Required picker case did not pass: empty_indexed_search_does_not_connect_and_closing_cancels_inflight_work'}
 if(-not$out.Contains('test picker::tests::caller_options_remain_bounded_and_cannot_push_cancel_or_open_offscreen ... ok')){throw 'Required picker case did not pass: caller_options_remain_bounded_and_cannot_push_cancel_or_open_offscreen'}
 if(-not$out.Contains('test picker::tests::filtered_out_typed_names_are_rejected_and_save_confirmation_never_changes_a_file ... ok')){throw 'Required picker case did not pass: filtered_out_typed_names_are_rejected_and_save_confirmation_never_changes_a_file'}
 if(-not$out.Contains('test picker::tests::multiselect_deselection_keeps_filename_and_preview_on_a_selected_file ... ok')){throw 'Required picker case did not pass: multiselect_deselection_keeps_filename_and_preview_on_a_selected_file'}
 if(-not$out.Contains('test picker::tests::rendered_picker_keeps_files_and_actions_visible_and_opens_the_clicked_file ... ok')){throw 'Required picker case did not pass: rendered_picker_keeps_files_and_actions_visible_and_opens_the_clicked_file'}
 if(-not$out.Contains('test picker::tests::rendered_read_only_preview_does_not_cover_actions_or_change_files ... ok')){throw 'Required picker case did not pass: rendered_read_only_preview_does_not_cover_actions_or_change_files'}
 if(-not$out.Contains('test picker::tests::typed_hidden_folders_remain_navigable_with_an_active_file_filter ... ok')){throw 'Required picker case did not pass: typed_hidden_folders_remain_navigable_with_an_active_file_filter'}
 if(-not$out.Contains('test picker::tests::keyboard_navigation_after_a_pointer_click_opens_the_next_file ... ok')){throw 'Required picker case did not pass: keyboard_navigation_after_a_pointer_click_opens_the_next_file'}
 if(-not$out.Contains('test picker::tests::text_preview_uses_the_readable_shared_body_instead_of_a_thumbnail ... ok')){throw 'Required picker case did not pass: text_preview_uses_the_readable_shared_body_instead_of_a_thumbnail'}
 if(-not$out.Contains('test picker::tests::location_shortcut_replaces_the_path_on_repeated_navigation ... ok')){throw 'Required picker case did not pass: location_shortcut_replaces_the_path_on_repeated_navigation'}
 if(-not$out.Contains('test picker::tests::location_shortcut_uses_event_modifiers_when_the_final_snapshot_has_released_ctrl ... ok')){throw 'Required picker case did not pass: location_shortcut_uses_event_modifiers_when_the_final_snapshot_has_released_ctrl'}
 if(-not$out.Contains('test picker::tests::folder_only_picker_preserves_file_preview_preference_without_starting_its_workers ... ok')){throw 'Required picker case did not pass: folder_only_picker_preserves_file_preview_preference_without_starting_its_workers'}
 if(-not$out.Contains('test picker::tests::caller_location_overrides_remembered_folder_without_resetting_picker_options ... ok')){throw 'Required picker case did not pass: caller_location_overrides_remembered_folder_without_resetting_picker_options'}
 if(-not$out.Contains('test internal_picker::tests::parent_owned_folder_picker_has_visible_actions_and_updates_only_the_requested_roots ... ok')){throw 'Required internal picker case did not pass: internal_picker::tests::parent_owned_folder_picker_has_visible_actions_and_updates_only_the_requested_roots'}
 if(-not$out.Contains('test internal_picker::tests::picker_escape_cancels_without_forwarding_keys_to_the_parent_or_writing_a_file ... ok')){throw 'Required internal picker case did not pass: internal_picker::tests::picker_escape_cancels_without_forwarding_keys_to_the_parent_or_writing_a_file'}
 if(-not$out.Contains('test internal_picker::tests::secondary_window_picker_keeps_the_main_window_from_consuming_its_location_shortcut ... ok')){throw 'Required internal picker case did not pass: internal_picker::tests::secondary_window_picker_keeps_the_main_window_from_consuming_its_location_shortcut'}
 if(-not$out.Contains('test picker_preferences::tests::picker_preferences_round_trip_without_changing_main_configuration ... ok')){throw 'Required internal picker case did not pass: picker_preferences::tests::picker_preferences_round_trip_without_changing_main_configuration'}
 if(-not$out.Contains('test config::tests::removing_preferences_clears_main_and_picker_files_and_preserves_other_data ... ok')){throw 'Required internal picker case did not pass: config::tests::removing_preferences_clears_main_and_picker_files_and_preserves_other_data'}
 if(-not$out.Contains('test file_grid::tests::grid_cells_render_separate_semantic_icon_and_label_colors_and_themed_corners ... ok')){throw 'Required internal picker case did not pass: file_grid::tests::grid_cells_render_separate_semantic_icon_and_label_colors_and_themed_corners'}
 if(-not$out.Contains('test preview_render::tests::wrapped_preview_footer_stays_inside_its_viewport ... ok')){throw 'Required footer case did not pass: wrapped_preview_footer_stays_inside_its_viewport'}
 if(-not$out.Contains('test preview_render::tests::short_preview_preserves_its_footer_without_forcing_body_height ... ok')){throw 'Required footer case did not pass: short_preview_preserves_its_footer_without_forcing_body_height'}
 if(-not$out.Contains('test preview_render::tests::archive_and_document_fallback_footers_fit_narrow_viewports ... ok')){throw 'Required footer case did not pass: archive_and_document_fallback_footers_fit_narrow_viewports'}
 if(-not$out.Contains('test preview_render::tests::spreadsheet_footer_and_omission_notes_fit_narrow_viewports ... ok')){throw 'Required footer case did not pass: spreadsheet_footer_and_omission_notes_fit_narrow_viewports'}
 if(-not$out.Contains('test preview_render::tests::structured_document_footer_fits_below_its_fallback_control ... ok')){throw 'Required footer case did not pass: structured_document_footer_fits_below_its_fallback_control'}
 if(-not$out.Contains('test preview_render::tests::expanded_extracted_text_keeps_its_footer_visible ... ok')){throw 'Required footer case did not pass: expanded_extracted_text_keeps_its_footer_visible'}
 if(-not$out.Contains('test preview_render::tests::shared_preview_bodies_have_one_scrollbar_and_stationary_footers ... ok')){throw 'Required footer case did not pass: shared_preview_bodies_have_one_scrollbar_and_stationary_footers'}
}finally{if($p-and-not$p.HasExited){& C:\Windows\System32\taskkill.exe /PID $p.Id /T /F|Out-Null;$null=$p.WaitForExit(10000)};Remove-Item -LiteralPath $homeRoot -Recurse -Force}

} catch {
 $receipt=@{phase=$phase;exception=$_.Exception.ToString();scriptStack=$_.ScriptStackTrace}
 [IO.File]::WriteAllText("C:\OttrinJobFixtureQA\results\$phase-actor-error.json",($receipt|ConvertTo-Json -Depth 8))
 throw
}
