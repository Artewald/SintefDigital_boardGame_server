use std::ops::ControlFlow;

use game_core::{
    rule_checker::{RuleChecker},
    game_data::{structs::{
        gamestate::GameState, player_input::PlayerInput, edge_restriction::EdgeRestriction, neighbour_relationship::NeighbourRelationship},
        enums::{player_input_type::PlayerInputType, district_modifier_type::DistrictModifierType, restriction_type::RestrictionType, in_game_id::InGameID},
        custom_types::{NodeID, ErrorData}}};

type RuleFn = Box<dyn Fn(&GameState, &PlayerInput) -> ValidationResponse<String> + Send + Sync>;

struct Rule {
    pub related_inputs: Vec<PlayerInputType>,
    pub rule_fn: RuleFn,
}

/// This struct contains the implementation of the RuleChecker trait.
/// It contains a list of rules that are checked when a player input is received.
pub struct GameRuleChecker {
    rules: Vec<Rule>,
}

enum ValidationResponse<T> {
    Valid,
    Invalid(T),
}

impl RuleChecker for GameRuleChecker {
    /// Checks if the input is valid based on the rules defined by this `GameRuleChecker`.
    fn is_input_valid(&self, game: &GameState, player_input: &PlayerInput) -> Option<ErrorData> {
        let mut error_str = "Invalid input!".to_string();
        let foreach_status = &self.rules.iter().try_for_each(|rule| {
            if rule.related_inputs.iter().all(|input_type| {
                input_type != &player_input.input_type && input_type != &PlayerInputType::All
            }) {
                return ControlFlow::Continue(());
            }

            match (rule.rule_fn)(game, player_input) {
                ValidationResponse::Valid => ControlFlow::Continue(()),
                ValidationResponse::Invalid(e) => {
                    error_str = e;
                    ControlFlow::Break(false)
                }
            }
        });
        if foreach_status.eq(&ControlFlow::Break(false)) {
            return Some(error_str);
        }
        None
    }
}

impl Default for GameRuleChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl GameRuleChecker {
    /// Creates a new GameRuleChecker based on the rules defined by it.
    #[must_use]
    pub fn new() -> Self {
        Self {
            rules: Self::get_rules(),
        }
    }

    fn get_rules() -> Vec<Rule> {
        let game_started = Rule {
            related_inputs: vec![
                PlayerInputType::Movement,
                PlayerInputType::ModifyDistrict,
                PlayerInputType::NextTurn,
                PlayerInputType::UndoAction,
            ],
            rule_fn: Box::new(has_game_started),
        };
        let players_turn = Rule {
            related_inputs: vec![PlayerInputType::All],
            rule_fn: Box::new(is_players_turn),
        };
        let orchestrator_check = Rule {
            related_inputs: vec![
                PlayerInputType::StartGame,
                PlayerInputType::ModifyEdgeRestrictions,
                PlayerInputType::ModifyDistrict,
            ],
            rule_fn: Box::new(is_orchestrator),
        };
        let player_has_position = Rule {
            related_inputs: vec![PlayerInputType::Movement],
            rule_fn: Box::new(has_position),
        };
        let toggle_bus = Rule {
            related_inputs: vec![PlayerInputType::SetPlayerBusBool],
            rule_fn: Box::new(can_toggle_bus),
        };
        let next_to_node = Rule {
            related_inputs: vec![PlayerInputType::Movement],
            rule_fn: Box::new(next_node_is_neighbour),
        };
        let enough_moves = Rule {
            related_inputs: vec![PlayerInputType::Movement],
            rule_fn: Box::new(has_enough_moves),
        };
        let move_to_node = Rule {
            related_inputs: vec![PlayerInputType::Movement],
            rule_fn: Box::new(can_move_to_node),
        };
        let can_modify_edge_restriction = Rule {
            related_inputs: vec![PlayerInputType::ModifyEdgeRestrictions],
            rule_fn: Box::new(is_edge_modification_action_valid),
        };

        let rules = vec![
            game_started,
            players_turn,
            orchestrator_check,
            player_has_position,
            toggle_bus,
            next_to_node,
            enough_moves,
            move_to_node,
            can_modify_edge_restriction,
        ];
        rules
    }
}

// ================== MACROS ====================
macro_rules! get_player_or_return_invalid_response {
    ($game:expr, $player_input:expr) => {{
        let player_result = $game.get_player_with_unique_id($player_input.player_id);
        let player = match player_result {
            Ok(p) => p,
            Err(e) => return ValidationResponse::Invalid(e.to_string()),
        };
        player.clone()
    }};
}

macro_rules! get_player_position_id_or_return_invalid_response {
    ($player:expr) => {{
        match $player.position_node_id {
            Some(id) => id,
            None => return ValidationResponse::Invalid("The player does not have a position and can therefore not check if it's a valid action!".to_string()),
        }
    }};
}

// ==================== RULES ====================
// If you are unsure what the code does/checks, it can be smart to check what the errors that can be returned are.

fn has_game_started(game: &GameState, _player_input: &PlayerInput) -> ValidationResponse<String> {
    match game.is_lobby {
        true => ValidationResponse::Invalid("The game has not started yet!".to_string()),
        false => ValidationResponse::Valid,
    }
}

fn has_enough_moves(game: &GameState, player_input: &PlayerInput) -> ValidationResponse<String> {
    let player = get_player_or_return_invalid_response!(game, player_input);

    if player.remaining_moves == 0 {
        return ValidationResponse::Invalid("The player has no remaining moves!".to_string());
    }

    let Some(related_node_id) = player_input.related_node_id else {
        return ValidationResponse::Invalid("There was no node to get cost to!".to_string());
    };

    let mut game_clone = game.clone();

    match game_clone.move_player_with_id(player_input.player_id, related_node_id) {
        Ok(_) => (),
        Err(e) => return ValidationResponse::Invalid(e),
    }

    has_non_negative_amount_of_moves_left(&game_clone, player_input)
}

// Checks if the player has non-negative amount of remaining moves in the provided GameState.
fn has_non_negative_amount_of_moves_left(
    game: &GameState,
    player_input: &PlayerInput,
) -> ValidationResponse<String> {
    let player = get_player_or_return_invalid_response!(game, player_input);

    if player.remaining_moves < 0 {
        return ValidationResponse::Invalid(
            format!("The player does not have enough remaining moves! The player would have {} remaining moves!", player.remaining_moves),
        );
    }

    ValidationResponse::Valid
}

// Checks if the player can enter the district the player wants to move to based on their objective card/vehicle type.
fn can_enter_district(game: &GameState, player_input: &PlayerInput) -> ValidationResponse<String> {
    let player = get_player_or_return_invalid_response!(game, player_input);

    let district_modifiers = &game.district_modifiers;

    let player_objective_card = match &player.objective_card {
        Some(objective_card) => objective_card,
        None => {
            return ValidationResponse::Invalid(
                "Error: Player does not have an objective card".to_string(),
            )
        }
    };

    let neighbours = match player.position_node_id {
        Some(pos) => match game.map.get_neighbour_relationships_of_node_with_id(pos) {
            Some(vec) => vec,
            None => {
                return ValidationResponse::Invalid(format!(
                    "Error: Node with ID {} does not exist",
                    pos
                ))
            }
        },
        None => {
            return ValidationResponse::Invalid(
                "Error: Player does not have a valid position and can therefore not move"
                    .to_string(),
            )
        }
    };

    let Some(to_node_id) = player_input.related_node_id else {
        return ValidationResponse::Invalid("Error: Related node ID does not exist in player input and has to be set for player movement".to_string());
    };
    let Some(neighbour_relationship) = neighbours.iter().find(|neighbour| neighbour.to == to_node_id) else {
        return ValidationResponse::Invalid("Error: There is no neighbouring node with the ID given".to_string());
    };

    let mut district_has_modifier = false;
    for dm in district_modifiers {
        if dm.district != neighbour_relationship.neighbourhood
            || dm.modifier != DistrictModifierType::Access
        {
            continue;
        }
        let Some(vehicle_type) = dm.vehicle_type else {
            return ValidationResponse::Invalid("Error: There was no vehicle for access modifier".to_string());
        };
        district_has_modifier = true;
        if player_objective_card
            .special_vehicle_types
            .contains(&vehicle_type)
            || (vehicle_type == RestrictionType::Destination
            && GameState::player_has_objective_in_district(&game.map, &player, dm.district))
        {
            return ValidationResponse::Valid;
        }
    }

    if !district_has_modifier {
        return ValidationResponse::Valid;
    }
    ValidationResponse::Invalid(
        format!("Invalid move: Player does not have required vehicle type to access this district. District modifiers: {:?}", district_modifiers),
    )
}

fn has_position(game: &GameState, player_input: &PlayerInput) -> ValidationResponse<String> {
    match game.get_player_with_unique_id(player_input.player_id) {
        Ok(p) => {
            if p.position_node_id.is_none() {
                return ValidationResponse::Invalid(
                    "The player does not have a position!".to_string(),
                );
            }
            ValidationResponse::Valid
        }
        Err(e) => ValidationResponse::Invalid(e.to_string()),
    }
}

fn next_node_is_neighbour(
    game: &GameState,
    player_input: &PlayerInput,
) -> ValidationResponse<String> {
    match game.get_player_with_unique_id(player_input.player_id) {
        Ok(p) => {
            match p.position_node_id {
                Some(node_id) => {
                    let Some(related_node_id) = player_input.related_node_id else {
                        return ValidationResponse::Invalid("There was node to check if it's a neighbour!".to_string());
                    };
                    let are_neighbours =
                        match game.map.are_nodes_neighbours(node_id, related_node_id) {
                            Ok(b) => b,
                            Err(e) => return ValidationResponse::Invalid(e),
                        };
                    if !are_neighbours {
                        return ValidationResponse::Invalid(format!(
                            "The node {related_node_id} is not a neighbour of the player's position!",
                        ));
                    }
                }
                None => {
                    return ValidationResponse::Invalid(
                        "The player does not have a position!".to_string(),
                    )
                }
            }
            ValidationResponse::Valid
        }
        Err(e) => ValidationResponse::Invalid(e.to_string()),
    }
}

fn is_players_turn(game: &GameState, player_input: &PlayerInput) -> ValidationResponse<String> {
    if game.is_lobby || player_input.input_type == PlayerInputType::LeaveGame {
        return ValidationResponse::Valid;
    }

    let player = get_player_or_return_invalid_response!(game, player_input);

    if game.current_players_turn != player.in_game_id {
        return ValidationResponse::Invalid("It's not the current players turn".to_string());
    }

    ValidationResponse::Valid
}

fn is_orchestrator(game: &GameState, player_input: &PlayerInput) -> ValidationResponse<String> {
    let player = get_player_or_return_invalid_response!(game, player_input);
    if player.in_game_id != InGameID::Orchestrator {
        return ValidationResponse::Invalid(
            "The player is not the orchestrator of the game!".to_string(),
        );
    }

    ValidationResponse::Valid
}

// Checks if the player is allowed to modify the edge they are trying to modify.
#[allow(unused_variables)]
fn is_edge_modification_action_valid(
    game: &GameState,
    player_input: &PlayerInput,
) -> ValidationResponse<String> {
    let Some(edge_mod) = player_input.edge_modifier.clone() else {
        return ValidationResponse::Invalid("There was no modifier on the edge modifier player input, and can therefore not check the input further!".to_string());
    };

    let Some(neighbours_one) = game.map.get_neighbour_relationships_of_node_with_id(edge_mod.node_one) else {
        return ValidationResponse::Invalid(format!("The node {} does not have neighbours and can therefore not have restrictions!", edge_mod.node_one));
    };

    let Some(neighbours_two) = game.map.get_neighbour_relationships_of_node_with_id(edge_mod.node_two) else {
        return ValidationResponse::Invalid(format!("The node {} does not have neighbours and can therefore not have restrictions!", edge_mod.node_one));
    };

    default_can_modify_edge_restriction(&edge_mod, &neighbours_one, edge_mod.node_two)

    // match edge_mod.edge_restriction { // This can be turned on if you only want to add or delete edges next to park and ride start node or other park and ride edges, but you cannot delete edges if there are cycles.
    //     RestrictionType::ParkAndRide => can_modify_park_and_ride(game, &edge_mod, &neighbours_one, &neighbours_two), 
    //     _ => default_can_modify_edge_restriction(&edge_mod, &neighbours_one, edge_mod.node_two),
    // }

}

fn default_can_modify_edge_restriction(edge_mod: &EdgeRestriction, neighbours_one: &[NeighbourRelationship], node_two_id: NodeID) -> ValidationResponse<String> {
    let Some(relationship) = neighbours_one.iter().find(|relationship| relationship.to == node_two_id) else {
        return ValidationResponse::Invalid(format!("The node {} does not have a neighbour with id {}!", edge_mod.node_one, node_two_id));
    };
    if edge_mod.delete {
        if relationship.is_modifiable {
            return ValidationResponse::Valid;
        }
        return ValidationResponse::Invalid(format!("A edge restriction {:?} already exists on the edge between node {} and node {} or is not modifiable! Modifiable: {}", edge_mod.edge_restriction, edge_mod.node_one, edge_mod.node_two, relationship.is_modifiable));
    }
    else if !relationship.is_modifiable {
        return ValidationResponse::Invalid(format!("The edge between node {} and node {} or is not modifiable!", edge_mod.node_one, edge_mod.node_two));
    }
    ValidationResponse::Valid
}

#[allow(dead_code)]
fn can_modify_park_and_ride(game: &GameState, park_and_ride_mod: &EdgeRestriction, neighbours_one: &[NeighbourRelationship], neighbours_two: &[NeighbourRelationship]) -> ValidationResponse<String> {
    if park_and_ride_mod.delete {
        if neighbours_one
            .iter()
            .filter(|neighbour| neighbour.restriction == Some(RestrictionType::ParkAndRide) && neighbour.is_modifiable)
            .count()
            < 2
            || neighbours_two
                .iter()
                .filter(|neighbour| neighbour.restriction == Some(RestrictionType::ParkAndRide) && neighbour.is_modifiable)
                .count()
                < 2
        {
            return ValidationResponse::Valid;
        }
        return ValidationResponse::Invalid("It's not possible to delete a park & ride edge that is connected to more than one other park & ride edge or the park & ride egde is not modifiable!".to_string());
    }

    let node_one = match game.map.get_node_by_id(park_and_ride_mod.node_one) {
        Ok(n) => n,
        Err(e) => {
            return ValidationResponse::Invalid(
                e + " and can therefore not check wether the park & ride can be placed here!",
            )
        }
    };

    let node_two = match game.map.get_node_by_id(park_and_ride_mod.node_two) {
        Ok(n) => n,
        Err(e) => {
            return ValidationResponse::Invalid(
                e + " and can therefore not check wether the park & ride can be placed here!",
            )
        }
    };

    if node_one.is_parking_spot || node_two.is_parking_spot {
        return ValidationResponse::Valid;
    }

    if neighbours_one
        .iter()
        .filter(|neighbour| neighbour.restriction == Some(RestrictionType::ParkAndRide))
        .count()
        > 0
        || neighbours_two
            .iter()
            .filter(|neighbour| neighbour.restriction == Some(RestrictionType::ParkAndRide))
            .count()
            > 0
    {
        return ValidationResponse::Valid;
    }

    ValidationResponse::Invalid(format!("Cannot place park & ride on the edge between node with ids {} and {} because there is no adjacent parking spots or park and ride edges!", park_and_ride_mod.node_one, park_and_ride_mod.node_two))
}

fn can_move_to_node(game: &GameState, player_input: &PlayerInput) -> ValidationResponse<String> {
    let player = get_player_or_return_invalid_response!(game, player_input);
    
    let player_pos = get_player_position_id_or_return_invalid_response!(player);

    let Some(to_node_id) = player_input.related_node_id else {
        return ValidationResponse::Invalid("There is no related node to the movement input. There needs to be a node if a players should move!".to_string());
    };

    let Some(neighbours) = game.map.get_neighbour_relationships_of_node_with_id(player_pos) else {
        return ValidationResponse::Invalid(format!("The node {} does not have neighbours and can therefore not have park and ride!", player_pos));
    };

    if player.is_bus {
        if neighbours
            .iter()
            .any(|neighbour| neighbour.restriction == Some(RestrictionType::ParkAndRide) && neighbour.to == to_node_id)
        {
            return ValidationResponse::Valid;
        }
        return ValidationResponse::Invalid(
            format!("The player cannot move here because the node (with id {}) is not a neighbouring node connected with a park & ride edge!", to_node_id),
        );
    }

    let current_node = match game.map.get_node_by_id(player_pos) {
        Ok(n) => n,
        Err(e) => {
            return ValidationResponse::Invalid(
                e + " And can therefore not check whether the player can move here!",
            )
        }
    };

    let to_node = match game.map.get_node_by_id(to_node_id) {
        Ok(n) => n,
        Err(e) => {
            return ValidationResponse::Invalid(
                e + " And can therefore not check whether the player can move here!",
            )
        }
    };

    if current_node.is_connected_to_rail && to_node.is_connected_to_rail && !player.is_bus {
        if neighbours
            .iter()
            .any(|neighbour| neighbour.is_connected_through_rail && neighbour.to == to_node_id)
        {
            return ValidationResponse::Valid;
        }
        return ValidationResponse::Invalid(
            format!("The player cannot move here because the node (with id {}) is not a neighbouring node connected through the railway!", to_node_id),
        );
    }

    if (!current_node.is_connected_to_rail || !to_node.is_connected_to_rail) && neighbours.iter().any(|neighbour| neighbour.is_connected_through_rail && neighbour.to == to_node_id) {
        return ValidationResponse::Invalid(
            format!("The player cannot move here because the node (with id {}) is not a neighbouring node connected through the railway!", to_node_id),
        );
    }
    
    let Some(neighbour_relationship) = neighbours.iter().find(|neighbour| neighbour.to == to_node_id) else {
        return ValidationResponse::Invalid(format!("The node {} is not a neighbour of the node {} and can therefore not be moved to!", to_node_id, player_pos));
    };

    let Some(to_node_neighbours) = game.map.get_neighbour_relationships_of_node_with_id(to_node_id) else {
        return ValidationResponse::Invalid(format!("The node {} does not have neighbours and can therefore not have park and ride!", to_node_id));
    };

    if let Some(to_node_neighbour_to_self) = to_node_neighbours.iter().find(|neighbour| neighbour.to == player_pos) {
        if to_node_neighbour_to_self.restriction == Some(RestrictionType::OneWay) {
            return ValidationResponse::Invalid(format!("The player cannot move to node with id {} because it's a one way street in the opposite direction!", to_node_id));
        }
    };

    if let Some(restriction) = neighbour_relationship.restriction {
        let Some(objective_card) = &player.objective_card else {
            return ValidationResponse::Invalid(format!("The player {} does not have an objective card and we can therefore not check if the player has access to the given zone!", player.name));
        };

        if (!(objective_card.special_vehicle_types.contains(&restriction)
        || (restriction == RestrictionType::Destination
        && GameState::player_has_objective_in_district(&game.map, &player, neighbour_relationship.neighbourhood)))) && restriction != RestrictionType::OneWay
         {
            return ValidationResponse::Invalid(format!("The player {} does not have access to the edge {:?} and can therefore not move to the node {}!", player.name, restriction, to_node_id));
        }

        return ValidationResponse::Valid;
    }

    match can_enter_district(game, player_input) {
        ValidationResponse::Valid => (),
        ValidationResponse::Invalid(e) => return ValidationResponse::Invalid(e),
    }

    if neighbours
        .iter()
        .any(|neighbour| neighbour.restriction == Some(RestrictionType::ParkAndRide) && neighbour.to == to_node_id)
    {
        return ValidationResponse::Invalid(
            "The player cannot move here because it's a park & ride edge!".to_string(),
        );
    }

    ValidationResponse::Valid
}

fn can_toggle_bus(game: &GameState, player_input: &PlayerInput) -> ValidationResponse<String> {
    let player = get_player_or_return_invalid_response!(game, player_input);
    
    let Some(_) = player_input.related_bool else {
        return ValidationResponse::Invalid("Could not check if you can toggle bus because the related bool was not set. It's needed for so that we can know if you want to stop being a bus or change to a bus!".to_string());
    };

    let player_pos = get_player_position_id_or_return_invalid_response!(player);
    let node = match game.map.get_node_by_id(player_pos) {
        Ok(n) => n,
        Err(e) => {
            return ValidationResponse::Invalid(
                e + " and can therefore not check wether the player can toggle bus!",
            )
        }
    };

    if !node.is_parking_spot {
        return ValidationResponse::Invalid(
            "You cannot toggle bus if you are not on a parking spot!".to_string(),
        );
    }

    ValidationResponse::Valid
}
